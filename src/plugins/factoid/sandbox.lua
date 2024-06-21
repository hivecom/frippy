function send(text)
	local text = tostring(text)
	local len = #output
	if len < 1 then
		output = { text }
	else
		output[len] = output[len] .. text
	end
end

function sendln(text)
	send(text)
	table.insert(output, "")
end

function trim(s)
	local from = s:match("^%s*()")
	return from > #s and "" or s:match(".*%S", from)
end

trimmedInput = trim(input)

if trimmedInput == "" then
	ioru = user
else
	ioru = trimmedInput
end

local sandbox_env = {
	print = send,
	println = sendln,
	trim = trim,
	eval = nil,
	sleep = nil,
	json = { decode = json_decode, encode = json_encode },
	date = { parse = parse_date },
	timestamp = { format = format_timestamp },
	retrieve = retrieve,
	persist = persist,
	args = args,
	input = input,
	user = user,
	ioru = ioru,
	channel = channel,
	request = download,
	string = string,
	math = math,
	table = table,
	pairs = pairs,
	ipairs = ipairs,
	next = next,
	select = select,
	unpack = unpack,
	tostring = tostring,
	tonumber = tonumber,
	type = type,
	assert = assert,
	error = error,
	pcall = pcall,
	xpcall = xpcall,
	_VERSION = _VERSION,
}

function split(inputstr, sep)
	if sep == nil then
		sep = "%s"
	end
	local t = {}
	for str in string.gmatch(inputstr, "([^" .. sep .. "]+)") do
		table.insert(t, str)
	end
	return t
end

sandbox_env.os = {
	date = os.date,
	time = os.time,
	difftime = os.difftime,
}

sandbox_env.string.rep = nil
sandbox_env.string.dump = nil
sandbox_env.string.split = split
sandbox_env.math.randomseed = nil

-- Temporary evaluation function
function eval(code)
	local c, e = load(code, nil, nil, sandbox_env)
	if c then
		return c()
	else
		error(e)
	end
end

-- Only sleeps for 1 second at a time
-- This ensures that the timeout check can still run
function safesleep(dur)
	while dur > 1000 do
		dur = dur - 1000
		sleep(1000)
	end
	sleep(dur)
end

sandbox_env.eval = eval
sandbox_env.sleep = safesleep

local f, e = load(factoid, nil, nil, sandbox_env)

if f then
	f()
else
	error(e)
end
