-- Typed server-lifecycle tooling for axoner-web processes.

local record M
   record Instance
      provider: string
      model: string
      port: integer
      pid: integer
      key: string
   end

   build_key: function(string, string, integer): string
   derive_port: function(string, string): integer
   parse_key: function(string): (string, string, integer)
   pid_path: function(string): string
   log_path: function(string): string
   is_running: function(integer): boolean
   up: function(string, string, integer): M.Instance
   down: function(string): boolean
   down_all: function(): integer
   status: function(): {M.Instance}
   logs: function(string): string
end

local root = (arg[0]:match("^(.*)/[^/]*$") or ".") .. "/.."
local RUN_DIR = root .. "/.tmp/run"
local BIN = root .. "/target/release/examples/axoner-web"
local READINESS_TIMEOUT_S = 10
local KILL_WAIT_S = 5

-- Lua 5.1/LuaJIT os.execute returns a numeric exit status; 5.2+ returns a
-- boolean. Normalize to an integer status so callers can compare against 0.
local function shell_status(cmd: string): integer
   local res: any = os.execute(cmd)
   if type(res) == "number" then
      return res as integer
   end
   if res == true then
      return 0
   end
   return 1
end

function M.build_key(provider: string, model: string, port: integer): string
   local model_key = (model:gsub("/", "_"))
   return string.format("%s--%s--%d", provider, model_key, port)
end

-- Deterministic port so tests/evals can find the server: 9300 + djb2 hash of
-- "provider--model-with-slashes-as-underscores" mod 500 (range 9300-9799).
function M.derive_port(provider: string, model: string): integer
   local model_key = (model:gsub("/", "_"))
   local base = provider .. "--" .. model_key
   local h = 5381
   for i = 1, #base do
      h = (h * 33 + string.byte(base, i)) % 2147483647
   end
   return 9300 + h % 500
end

function M.parse_key(key: string): (string, string, integer)
   local provider, rest = key:match("^(.-)%-%-(.+)$")
   if not provider or not rest then
      return "", "", 0
   end
   local model, port_s = rest:match("^(.-)%-%-(%d+)$")
   if not model or not port_s then
      return provider, rest, 0
   end
   return provider, model, (tonumber(port_s) or 0) as integer
end

function M.pid_path(key: string): string
   return RUN_DIR .. "/" .. key .. ".pid"
end

function M.log_path(key: string): string
   return RUN_DIR .. "/" .. key .. ".log"
end

function M.is_running(pid: integer): boolean
   return shell_status("kill -0 " .. tostring(pid) .. " 2>/dev/null") == 0
end

local function read_pid(key: string): integer
   local f = io.open(M.pid_path(key), "r")
   if not f then
      return 0
   end
   local line = f:read("*l")
   f:close()
   return (tonumber(line or "") or 0) as integer
end

local function instance_for(key: string, pid: integer): M.Instance
   local provider, model, port = M.parse_key(key)
   return {
      provider = provider,
      model = model,
      port = port,
      pid = pid,
      key = key,
   }
end

local function list_pid_keys(): {string}
   local keys: {string} = {}
   local pipe = io.popen("ls " .. RUN_DIR .. "/*.pid 2>/dev/null")
   if not pipe then
      return keys
   end
   for path in pipe:lines() do
      local key = path:match("([^/]+)%.pid$")
      if key then
         table.insert(keys, key)
      end
   end
   pipe:close()
   return keys
end

function M.up(provider: string, model: string, port: integer): M.Instance
   local key = M.build_key(provider, model, port)
   local existing = read_pid(key)
   if existing > 0 and M.is_running(existing) then
      return instance_for(key, existing)
   end

   local probe = io.open(BIN, "r")
   if not probe then
      io.stderr:write("ERROR: " .. BIN .. " not found. Run: make build-server\n")
      os.exit(1)
   end
   probe:close()

   os.execute("mkdir -p " .. RUN_DIR)
   local log = M.log_path(key)
   local cmd = "nohup sh -c 'cd "
      .. root
      .. " && exec "
      .. BIN
      .. " serve --port "
      .. tostring(port)
      .. " --provider "
      .. provider
      .. " --model "
      .. model
      .. "' > "
      .. log
      .. " 2>&1 & echo $!"
   local pipe = io.popen(cmd)
   if not pipe then
      io.stderr:write("ERROR: failed to spawn server for " .. key .. "\n")
      os.exit(1)
   end
   local pid_line = pipe:read("*l")
   pipe:close()
   local pid = (tonumber(pid_line or "") or 0) as integer
   if pid <= 0 then
      io.stderr:write("ERROR: could not determine spawned pid for " .. key .. "\n")
      os.exit(1)
   end

   local waited: number = 0
   while waited < READINESS_TIMEOUT_S do
      os.execute("sleep 0.25")
      waited = waited + 0.25
      if shell_status("curl -s -o /dev/null http://127.0.0.1:" .. tostring(port) .. "/") == 0 then
         local pf = io.open(M.pid_path(key), "w")
         if not pf then
            io.stderr:write("ERROR: cannot write pidfile " .. M.pid_path(key) .. "\n")
            os.exit(1)
         end
         pf:write(tostring(pid) .. "\n")
         pf:close()
         return instance_for(key, pid)
      end
   end

   os.execute("kill -9 " .. tostring(pid) .. " 2>/dev/null")
   io.stderr:write("ERROR: server for " .. key .. " not ready after " .. tostring(READINESS_TIMEOUT_S) .. "s; see " .. log .. "\n")
   os.exit(1)
end

function M.down(key: string): boolean
   local pid = read_pid(key)
   if pid <= 0 or not M.is_running(pid) then
      os.remove(M.pid_path(key))
      return false
   end
   os.execute("kill " .. tostring(pid) .. " 2>/dev/null")
   local waited: number = 0
   while M.is_running(pid) and waited < KILL_WAIT_S do
      os.execute("sleep 0.1")
      waited = waited + 0.1
   end
   if M.is_running(pid) then
      os.execute("kill -9 " .. tostring(pid) .. " 2>/dev/null")
   end
   os.remove(M.pid_path(key))
   return true
end

function M.down_all(): integer
   local count = 0
   for _, key in ipairs(list_pid_keys()) do
      if M.down(key) then
         count = count + 1
      end
   end
   return count
end

function M.status(): {M.Instance}
   local out: {M.Instance} = {}
   for _, key in ipairs(list_pid_keys()) do
      local pid = read_pid(key)
      if pid > 0 and M.is_running(pid) then
         table.insert(out, instance_for(key, pid))
      end
   end
   return out
end

function M.logs(key: string): string
   local pipe = io.popen("tail -n 40 " .. M.log_path(key) .. " 2>/dev/null")
   if not pipe then
      return ""
   end
   local lines: {string} = {}
   for line in pipe:lines() do
      table.insert(lines, line)
   end
   pipe:close()
   return table.concat(lines, "\n")
end

return M
