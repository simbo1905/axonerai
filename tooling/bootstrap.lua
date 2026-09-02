-- Resolve the LuaJIT (5.1) rocks tree without a pre-sourced LUA_PATH,
-- activate the Teal loader, and make sibling modules requirable from any cwd.

local here = arg[0]:match("^(.*)/[^/]*$") or "."

local function lr(which)
   local pipe = io.popen("luarocks --lua-version=5.1 path --lr-" .. which .. " 2>/dev/null")
   local out = pipe and pipe:read("*l") or nil
   if pipe then pipe:close() end
   return out
end

if not pcall(require, "tl") then
   local p, c = lr("path"), lr("cpath")
   if p and #p > 0 then package.path = p .. ";" .. package.path end
   if c and #c > 0 then package.cpath = c .. ";" .. package.cpath end
end

local ok, tl = pcall(require, "tl")
if not ok then
   io.stderr:write("ERROR: Teal (tl) not installed for the LuaJIT 5.1 ABI. Run: make init\n")
   os.exit(1)
end
tl.loader()

package.path = here .. "/?.lua;" .. here .. "/?.tl;" .. package.path

return here
