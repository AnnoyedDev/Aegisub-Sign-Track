local function english()
    local base = os.getenv("APPDATA") or os.getenv("XDG_CONFIG_HOME") or ((os.getenv("HOME") or "") .. "/.config")
    local f = io.open(base .. "/aegisigntrack/config.txt")
    local text = f and f:read("*a") or ""
    if f then f:close() end
    return text:find("langue=en", 1, true) ~= nil
end

local in_english = english()

local function T(fr, en)
    return in_english and en or fr
end

script_name = "AegiSignTrack"
script_description = T("Suit un mouvement de la vidéo avec AegiSignTrack et l'applique aux lignes sélectionnées", "Tracks a motion of the video with AegiSignTrack and applies it to the selected lines")
script_author = "AnnoyedDev"
script_version = "0.1.0"

local function config_file()
    return aegisub.decode_path("?user/aegisigntrack_path.txt")
end

local function tracker_path()
    local f = io.open(config_file())
    local path = f and f:read("*l")
    if f then f:close() end
    return (path and path ~= "") and path or "aegisigntrack"
end

local function fail(message)
    aegisub.log(0, message .. "\n")
    aegisub.cancel()
end

local function header(subs)
    local res_x, res_y, styles = nil, nil, {}
    for i = 1, #subs do
        local l = subs[i]
        if l.class == "info" and l.key == "PlayResX" then
            res_x = tonumber(l.value)
        elseif l.class == "info" and l.key == "PlayResY" then
            res_y = tonumber(l.value)
        elseif l.class == "style" then
            styles[l.name] = l
        elseif l.class == "dialogue" then
            break
        end
    end
    local video_w, video_h = aegisub.video_size()
    return res_x or video_w, res_y or video_h, styles
end

local function run(command)
    if package.config:sub(1, 1) ~= "\\" then
        return os.execute(command)
    end
    local quoted = '"' .. command .. '"'
    local ok, status = pcall(function()
        local ffi = require("ffi")
        pcall(ffi.cdef, [[
            int MultiByteToWideChar(unsigned int page, unsigned long flags, const char *text, int length, wchar_t *wide, int size);
            int _wsystem(const wchar_t *command);
        ]])
        local size = ffi.C.MultiByteToWideChar(65001, 0, quoted, -1, nil, 0)
        local wide = ffi.new("wchar_t[?]", size)
        ffi.C.MultiByteToWideChar(65001, 0, quoted, -1, wide, size)
        return ffi.C._wsystem(wide)
    end)
    if ok then
        return status
    end
    return os.execute(quoted)
end

local function track(subs, sel)
    local video = aegisub.project_properties().video_file
    if not video or video == "" or not aegisub.frame_from_ms(0) then
        fail(T("Chargez d'abord une vidéo dans Aegisub.", "Load a video in Aegisub first."))
    end
    local res_x, res_y, styles = header(subs)

    local first, last = math.huge, 0
    for _, i in ipairs(sel) do
        local l = subs[i]
        if l.class == "dialogue" then
            first, last = math.min(first, l.start_time), math.max(last, l.end_time)
        end
    end
    local start_frame, end_frame = aegisub.frame_from_ms(first), aegisub.frame_from_ms(last)
    if end_frame <= start_frame then
        fail(T("Les lignes sélectionnées ne couvrent aucune frame de la vidéo.", "The selected lines cover no frame of the video."))
    end

    local job_path, result_path = aegisub.decode_path("?temp/aegisigntrack_job.txt"), aegisub.decode_path("?temp/aegisigntrack_result.txt")
    os.remove(result_path)
    local job = assert(io.open(job_path, "w"))
    job:write("video=", video, "\nresult=", result_path, "\nres_x=", res_x, "\nres_y=", res_y, "\n")
    local folder, name = aegisub.decode_path("?script"), aegisub.file_name and aegisub.file_name() or ""
    if folder ~= "?script" and name ~= "" then
        job:write("script=", folder, "/", name, "\n")
    end
    local times = {}
    for f = start_frame, end_frame do
        times[#times + 1] = aegisub.ms_from_frame(f)
    end
    job:write("times=", table.concat(times, ","), "\n")
    local cuts = {}
    for _, k in ipairs(aegisub.keyframes and aegisub.keyframes() or {}) do
        if k > start_frame and k <= end_frame then
            cuts[#cuts + 1] = k - start_frame
        end
    end
    job:write("keyframes=", table.concat(cuts, ","), "\n")
    local sent = {}
    for _, i in ipairs(sel) do
        local l = subs[i]
        if l.class == "dialogue" then
            local s = styles[l.style] or {}
            if s.raw and not sent[l.style] then
                sent[l.style] = true
                job:write("style=", s.raw, "\n")
            end
            job:write("line=", table.concat({
                i,
                aegisub.frame_from_ms(l.start_time) - start_frame,
                aegisub.frame_from_ms(l.end_time) - start_frame,
                s.scale_x or 100, s.scale_y or 100, s.angle or 0, s.outline or 0, s.shadow or 0,
                l.raw,
            }, "\t"), "\n")
            job:write("box=", table.concat({ i, s.align or 2, s.fontsize or 0 }, "\t"), "\n")
        end
    end
    job:close()

    aegisub.progress.title("AegiSignTrack")
    aegisub.progress.task(T("Tracker ouvert : cliquez sur « Envoyer à Aegisub » quand le tracking est prêt.", "Tracker open: click “Send to Aegisub” when the tracking is ready."))
    local status = run(('"%s" --job "%s"'):format(tracker_path(), job_path))
    os.remove(job_path)

    local result = io.open(result_path)
    if not result then
        if status ~= 0 and status ~= true then
            fail(T("Impossible de lancer le tracker (", "Cannot start the tracker (") .. tracker_path() .. T(").\nIndiquez son emplacement avec Automatisme > AegiSignTrack > Chemin du tracker.", ").\nGive its location with Automation > AegiSignTrack > Tracker path."))
        end
        aegisub.cancel()
    end
    local generated, delete = {}, false
    for row in result:lines() do
        delete = delete or row == "original=delete"
        local kind, index, start_time, end_time, text = row:match("^(%a+)\t(%d+)\t(%d+)\t(%d+)\t(.*)$")
        if kind then
            index = tonumber(index)
            local line = subs[index]
            local layer, rest = text:match("^L(%d+)\t(.*)$")
            if layer then
                text = rest
                line.layer = line.layer + tonumber(layer)
            end
            line.start_time, line.end_time, line.text, line.comment = tonumber(start_time), tonumber(end_time), text, false
            generated[index] = generated[index] or { mask = {}, text = {} }
            table.insert(generated[index][kind], line)
        end
    end
    result:close()
    os.remove(result_path)

    local indexes = {}
    for index in pairs(generated) do
        indexes[#indexes + 1] = index
    end
    table.sort(indexes, function(a, b) return a > b end)
    local function insert(at, lines)
        for k, line in ipairs(lines) do
            if at + k - 1 > #subs then
                subs.append(line)
            else
                subs.insert(at + k - 1, line)
            end
        end
    end
    for _, index in ipairs(indexes) do
        local made = generated[index]
        if #made.text > 0 and delete then
            subs.delete(index)
            insert(index, made.text)
        elseif #made.text > 0 then
            local original = subs[index]
            original.comment = true
            subs[index] = original
            insert(index + 1, made.text)
        end
        insert(index, made.mask)
    end
    local new_sel, offset = {}, 0
    for k = #indexes, 1, -1 do
        local index, made = indexes[k], generated[indexes[k]]
        local gone = (delete and #made.text > 0) and 1 or 0
        for n = 0, #made.mask - 1 do
            new_sel[#new_sel + 1] = index + offset + n
        end
        for n = 1, #made.text do
            new_sel[#new_sel + 1] = index + offset + #made.mask + n - gone
        end
        offset = offset + #made.mask + #made.text - gone
    end
    aegisub.set_undo_point("AegiSignTrack")
    return new_sel
end

local function configure()
    local pressed, values = aegisub.dialog.display({
        { class = "label", x = 0, y = 0, label = T("Emplacement de l'exécutable aegisigntrack (vide : celui du PATH)", "Location of the aegisigntrack executable (empty: the one in the PATH)") },
        { class = "edit", name = "path", x = 0, y = 1, width = 40, text = tracker_path() == "aegisigntrack" and "" or tracker_path() },
    }, { T("Enregistrer", "Save"), T("Annuler", "Cancel") }, { ok = T("Enregistrer", "Save"), cancel = T("Annuler", "Cancel") })
    if pressed then
        local f = assert(io.open(config_file(), "w"))
        f:write(values.path, "\n")
        f:close()
    end
end

local function has_video(subs, sel)
    return #sel > 0 and aegisub.frame_from_ms(0) ~= nil
end

aegisub.register_macro(T("AegiSignTrack/Tracker la sélection", "AegiSignTrack/Track selection"), script_description, track, has_video)
aegisub.register_macro(T("AegiSignTrack/Chemin du tracker", "AegiSignTrack/Tracker path"), T("Indique où se trouve l'exécutable aegisigntrack", "Tells where the aegisigntrack executable is"), configure)
