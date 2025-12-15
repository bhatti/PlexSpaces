-- LuaTS Event Coordination (Lua)
-- Demonstrates Linda + Event-driven programming

local luats = require("luats")

-- Subscribe to events
luats.subscribe("data_ready", function(event)
    print("Received event:", event.payload)
end)

-- Publish event (also writes to TupleSpace)
luats.publish("data_ready", {
    payload = "processed_data",
    timestamp = os.time()
})

-- Read from TupleSpace (Linda pattern)
local tuple = luats.read({"event", "data_ready", _})

-- Write to TupleSpace
luats.write({"event", "data_ready", "data"})
