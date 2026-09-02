#!/usr/bin/env -S luajit
-- Entry point: manage local axoner-web server processes.
-- usage: tooling/serve.lua <up|down|down-all|status|logs> [provider] [model] [port]

dofile((arg[0]:match("^(.*)/[^/]*$") or ".") .. "/bootstrap.lua")

local serve = require("lib.serve")

local usage = [[
usage: tooling/serve.lua <command> [args]

commands:
  up <provider> <model> [port]     start server (idempotent); default port is
                                   deterministic: 9300 + hash(provider--model) % 500
  down <provider> <model> <port>   stop server by key, or: down all
  down-all                         stop every server with a pidfile in .tmp/run/
  status                           list live instances
  logs <provider> <model> <port>   tail (last 40 lines) a server's log
]]

local cmd = arg[1]

local function die(msg)
   io.stderr:write(msg .. "\n")
   os.exit(1)
end

if cmd == "up" then
   local provider, model = arg[2], arg[3]
   if not provider or not model then
      die(usage)
   end
   local port = arg[4] and tonumber(arg[4]) or serve.derive_port(provider, model)
   if not port or port <= 0 then
      die("ERROR: invalid port: " .. tostring(arg[4]))
   end
   local inst = serve.up(provider, model, port)
   print(string.format("up %s pid=%d url=http://127.0.0.1:%d/", inst.key, inst.pid, inst.port))
   os.exit(0)
end

if cmd == "down" then
   if arg[2] == "all" then
      local n = serve.down_all()
      print(string.format("stopped %d instance(s)", n))
      os.exit(0)
   end
   local provider, model = arg[2], arg[3]
   local port = arg[4] and tonumber(arg[4]) or serve.derive_port(provider, model)
   if not provider or not model or not port or port <= 0 then
      die(usage)
   end
   local key = serve.build_key(provider, model, port)
   if serve.down(key) then
      print("stopped " .. key)
   else
      print("not running " .. key)
   end
   os.exit(0)
end

if cmd == "down-all" then
   local n = serve.down_all()
   print(string.format("stopped %d instance(s)", n))
   os.exit(0)
end

if cmd == "status" then
   local instances = serve.status()
   if #instances == 0 then
      print("(no live instances)")
   end
   for _, inst in ipairs(instances) do
      print(string.format("%s pid=%d url=http://127.0.0.1:%d/", inst.key, inst.pid, inst.port))
   end
   os.exit(0)
end

if cmd == "logs" then
   local provider, model = arg[2], arg[3]
   local port = arg[4] and tonumber(arg[4]) or serve.derive_port(provider, model)
   if not provider or not model or not port or port <= 0 then
      die(usage)
   end
   local out = serve.logs(serve.build_key(provider, model, port))
   if #out > 0 then
      print(out)
   else
      print("(no log output)")
   end
   os.exit(0)
end

die(usage)
