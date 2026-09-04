# Scripting

FieldAssist embeds Lua 5.4. Scripts run in the **Script** panel (View → Script)
and from `init.lua` in the app config directory:

- macOS: `~/Library/Application Support/snd-review/init.lua`
- Windows: `%APPDATA%\snd-review\init.lua`
- Linux: `$XDG_CONFIG_HOME/snd-review/init.lua` (or `~/.config/snd-review/init.lua`)

`init.lua` is loaded once at startup. Use `app:on("loaded", function(c) ... end)`
there to run code whenever a composition is opened.

`print(...)` writes to the Script panel. Standard Lua libraries are available.

Times on the timeline are **sample indices** (frames), starting at `0`. Channel
indices are also 0-based.

## Globals

| Name | Description |
| --- | --- |
| `app` | The running application. Always present. |
| `print(...)` | Writes a line to the Script panel. |

There is no other host-provided global besides `app`. Open documents are reached
through `app.active` and `app.documents`.

## `app`

```lua
local c = app.active          -- composition, or nil if none is open
local all = app.documents     -- array of open compositions
local opened = app:open(path) -- open a file; returns the composition
app:command("edit.trim")      -- run a menu/keymap command by id
app:dofile("extra.lua")       -- execute a Lua file
app:on("loaded", function(c)  -- c is the composition that just loaded
  print("opened", c.name)
end)
```

`app:command` uses the same ids as the keymap (`file.open`, `transport.play_pause`,
`selection.add_marker`, …). Unknown ids are an error. See [Commands](#commands).

The only event name `app:on` accepts is `"loaded"`.

## Composition

`app.active` and each entry in `app.documents` is a **composition**. Edits,
markers, regions, and selection live on this object.

### Fields

| Field | Type | Notes |
| --- | --- | --- |
| `name` | string | Display title (file name when saved). |
| `path` | string or nil | Source or project path, if any. |
| `frames` | integer | Timeline length in samples. |
| `sample_rate` | integer | Hz. |
| `channels` | integer | Channel count. |
| `duration` | number | Length in seconds. |
| `position` | integer or nil | Playhead / caret sample. Assignable. |
| `selection` | Selection | Current selection. Assignable; `nil` clears it. |
| `regions` | array of Region | Named (or unlabeled) regions, in buffer order. |
| `markers` | array of Marker | User markers, ordered by frame then type. |

Reading `markers` or `regions` returns a snapshot table. Later adds/removes do
not update a table you already hold; read the field again.

### Selection methods

```lua
c:select(start, stop)              -- samples; all channels
c:select(start, stop, {0, 1})      -- specific channels
c:select_all()
c:clear_selection()
c.position = 44100
c.selection = { kind = "region", start = 0, stop = 100 }
c.selection = { kind = "position", start = 50 }
c.selection = nil                  -- same as clear_selection()
```

### Regions

```lua
local r = c:add_region({
  start = 0,
  stop = 44100,
  label = "intro",      -- optional
  channels = "all",     -- optional; "all" or {0, 1, ...}
})
print(r.id, r.start, r.stop, r.label)
c:remove_region(r.id)   -- returns whether a region was removed
```

### Markers

One marker of a given **type** may exist at a given frame. A second insert of
the same type at that frame is ignored and returns `nil`. Different types may
share a frame.

Built-in types: `"Blue"`, `"Yellow"`, `"Purple"`. The default type is `"Blue"`.
Custom type names are allowed if you pass a `color`.

```lua
-- table form
local m = c:add_marker({
  frame = 1000,           -- or sample = 1000
  type = "Yellow",        -- or kind; default "Blue"
  note = "door slam",     -- optional
  color = {1, 0, 0, 1},   -- optional RGBA in 0..1
})

-- positional form
c:add_marker(1000)             -- Blue at sample 1000
c:add_marker(1000, "Purple")

-- iterate
for _, marker in ipairs(c.markers) do
  print(marker.id, marker.frame, marker.type, marker.note)
end

-- lookup
local yellow = c:marker_at(1000, "Yellow")
local any = c:marker_at(1000)  -- first marker at that frame

-- delete
c:remove_marker(m)             -- marker object or integer id
c:remove_marker(m.id)
m:remove()
c:remove_marker_at(1000)           -- every type at that frame
c:remove_marker_at(1000, "Blue")   -- one type
```

`add_marker` returns the new Marker, or `nil` if that type already occupies the
frame.

### Timeline edits

These follow the current selection (or caret), same as the Edit menu:

| Method | Effect |
| --- | --- |
| `c:undo()` / `c:redo()` | History. `undo`/`redo` return whether a step ran. |
| `c:cut()` / `c:copy()` / `c:paste()` | Clipboard. |
| `c:delete()` | Replace the selection with silence (keep length). |
| `c:remove()` | Cut the selection out (timeline shrinks). |
| `c:duplicate()` | Duplicate the selection. |
| `c:trim()` | Trim the composition to the selection. |
| `c:roll(delta)` | Roll source by `delta` samples (negative = left). |

## Selection

`c.selection` is a snapshot object:

| Field | Type | Notes |
| --- | --- | --- |
| `kind` | `"none"`, `"position"`, or `"region"` | |
| `start` | integer or nil | Sample. For a position, start and stop are the same. |
| `stop` | integer or nil | Inclusive end sample for a region. |
| `channels` | `"all"` or array of integers | |

Assign a table with the same keys, a Selection object, or `nil`.

## Region

| Field | Type |
| --- | --- |
| `id` | integer |
| `start` | integer (sample) |
| `stop` | integer (sample, inclusive) |
| `channels` | `"all"` or array of integers |
| `label` | string or nil |

Fields are live: they read the current buffer. A region that has been removed
errors if you access its fields.

## Marker

| Field | Type |
| --- | --- |
| `id` | integer |
| `frame` | integer (sample). Alias: `sample`. |
| `type` | string (e.g. `"Blue"`) |
| `color` | array of four numbers `r, g, b, a` in `0..1` |
| `note` | string or nil |

| Method | Effect |
| --- | --- |
| `marker:remove()` | Delete this marker. Returns whether it still existed. |

Fields are live against the composition. After a successful `remove`, further
field access errors.

## Channels

Anywhere a channel scope is accepted (`select`, `add_region`, `selection.channels`):

- omit the argument, pass `nil`, or pass `"all"` — every channel
- pass `{0}` or `{0, 1}` — those channel indices

## Commands

`app:command(id)` runs the same actions as menus and key bindings:

**File:** `file.open`, `file.save`, `file.save_as`, `file.close`, `file.render`,
`file.quit`

**Help:** `help.about`

**View:** `view.fit_all`, `view.frame`, `view.zoom_in`, `view.zoom_out`,
`view.explorer`, `view.history`, `view.script`

**Transport:** `transport.home`, `transport.previous`, `transport.start`,
`transport.play_pause`, `transport.stop`, `transport.next`, `transport.end`,
`transport.loop`

**Edit:** `edit.undo`, `edit.redo`, `edit.cut`, `edit.copy`, `edit.paste`,
`edit.delete`, `edit.remove`, `edit.duplicate`, `edit.trim`, `edit.roll_left`,
`edit.roll_right`

**Selection / markers:** `selection.select_all`, `selection.select_none`,
`selection.invert`, `selection.marker_type_blue`, `selection.marker_type_yellow`,
`selection.marker_type_purple`, `selection.add_at_hover`, `selection.add_marker`,
`selection.delete_marker`

Marker commands use the active marker type and the Add at Hover setting from
the Selection menu. Prefer `c:add_marker` / `c:remove_marker` when the script
should choose the frame and type itself.

## Example

```lua
local c = app.active
if not c then
  return
end

c:select(0, c.frames - 1)
local intro = c:add_marker({
  frame = 0,
  type = "Blue",
  note = "start",
})
print("marker", intro.id, "at", intro.frame)

for _, marker in ipairs(c.markers) do
  print(marker.type, marker.frame, marker.note)
end
```
