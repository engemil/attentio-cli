# Shell Command Protocol

**NB!** Most of this is subject to change, as these things have not yet been implemented into the AttentioLight-1 firmware.

Command protocol specification for communication between the host CLI (`attentio`)
and the AttentioLight-1 (AL-1) device over USB.

The device firmware runs ChibiOS with its built-in shell module. Commands are
plain text, line-oriented — no binary serialization. The host sends a command
string, the device processes it and replies with a structured text response.

---

## Transport — USB CDC/ACM

The device exposes two USB CDC/ACM (virtual serial) interfaces over a single
USB connection.

| Property | Value |
|----------|-------|
| **VID** | `0x0483`* (STMicroelectronics / EngEmil.io) |
| **PID** | `0xDF11`* (AttentioLight-1) |
| **Baud rate** | 115200 (convention — ignored by USB, but required by serial APIs) |

(*) NB! To be changed in the future!

### Dual-CDC Interface Layout

| Interface | Role | Direction | Description |
|-----------|------|-----------|-------------|
| **CDC0** | Debug prints | Device -> Host (read-only) | Continuous diagnostic output |
| **CDC1** | Shell commands | Bidirectional (request/response) | Interactive command interface |

On the host, these appear as two serial ports (e.g. `/dev/ttyACM0` and
`/dev/ttyACM1`). The CLI identifies them by USB serial number and assigns
roles by port path order: lower path = CDC0, higher path = CDC1.

### Single-CDC Fallback

Firmware that exposes only one CDC interface is supported. The single port is treated as the shell command interface* (equivalent to CDC1). Debug print functionality is unavailable in this configuration.

(*) Might change in the future.

---

## CDC0 — Debug Print Stream

CDC0 carries a continuous, unstructured stream of debug/diagnostic output from
the firmware to the host.

### Format

- Free-form text lines, each terminated by `\r\n`
- No protocol framing — lines are raw debug output
- The host reads line-by-line and displays in real time

### Content

The firmware writes to CDC0 using `chprintf()` on the debug serial stream.
Typical output includes:

- Boot/init messages
- State transitions
- Diagnostic info (sensor readings, timing, etc.)
- Error/warning messages from firmware subsystems

### Example Output

NB! Not 100% accurate yet

```
[boot] AttentioLight-1 v1.0.0
[boot] Serial: ABC123
[led] Mode changed: solid -> pulse
[led] Brightness: 80
```

The exact format and content of debug lines is not standardized — it is
free-form and may change between firmware versions.

---

## CDC1 — Shell Command Protocol

CDC1 carries the structured command/response protocol used by the CLI to
control the device.

### Request Format

```
<command> [arguments...]\r\n
```

- A single line of plain ASCII text
- Terminated by `\r\n` (carriage return + line feed)
- Command and arguments are space-separated
- Quoted strings are supported for values containing spaces (e.g. `"My Light"`)

### Response Format

```
[payload line 1\r\n]
[payload line 2\r\n]
[...]
OK\r\n
```

Or on failure:

```
ERROR <message>\r\n
```

**Rules:**

- Zero or more payload lines may precede the terminator
- Each payload line is terminated by `\r\n`
- The response is concluded by exactly one of:
  - **`OK\r\n`** — command succeeded. Payload lines (if any) contain the result.
  - **`ERROR <message>\r\n`** — command failed. The message describes the error.
  - **`ERROR\r\n`** — command failed with no further detail.
- The host must read lines until a terminator is received or a timeout expires
- Host default timeout: **5 seconds**
- An unexpected connection close (EOF) is treated as a protocol error

### Sequence Diagram

```
  Host (CLI)                    Device (Firmware)
      |                               |
      |   "version\r\n"               |
      |------------------------------>|
      |                               |  (ChibiOS shell dispatches to handler)
      |           "1.0.0\r\n"         |
      |<------------------------------|
      |            "OK\r\n"           |
      |<------------------------------|
      |                               |
      |   "led color red\r\n"         |
      |------------------------------>|
      |            "OK\r\n"           |
      |<------------------------------|
      |                               |
      | "settings get nonexistent\r\n"|
      |------------------------------>|
      |   "ERROR unknown key\r\n"     |
      |<------------------------------|
      |                               |
```

---

## ChibiOS Shell Integration (Firmware Side)

The firmware uses the ChibiOS Shell module to handle command parsing and
dispatch on CDC1. This section describes how commands are registered and how
handlers should emit responses.

### Command Registration

Commands are registered as an array of `ShellCommand` entries, terminated by
a `{NULL, NULL}` sentinel:

```c
static const ShellCommand shell_commands[] = {
    {"version",  cmd_version},
    {"led",      cmd_led},
    {"settings", cmd_settings},
    {"dfu",      cmd_dfu},
    {NULL, NULL}
};
```

This array is passed to the shell configuration (`ShellConfig`) along with the
CDC1 serial stream.

### Handler Signature

Each handler receives:

```c
void cmd_example(BaseSequentialStream *chp, int argc, char *argv[]);
```

| Parameter | Description |
|-----------|-------------|
| `chp` | The serial stream to write responses to (CDC1) |
| `argc` | Number of arguments (excluding the command name itself) |
| `argv` | Argument strings (excluding the command name) |

### Writing Responses

Handlers write payload and terminators using `chprintf()`:

```c
void cmd_version(BaseSequentialStream *chp, int argc, char *argv[]) {
    (void)argc;
    (void)argv;

    chprintf(chp, "1.0.0\r\n");
    chprintf(chp, "OK\r\n");
}
```

For errors:

```c
void cmd_led(BaseSequentialStream *chp, int argc, char *argv[]) {
    if (argc < 2) {
        chprintf(chp, "ERROR missing arguments\r\n");
        return;
    }

    /* ... process command ... */

    chprintf(chp, "OK\r\n");
}
```

### Important Notes

- ChibiOS shell handles line buffering and tokenization of the incoming
  command — the handler receives pre-parsed arguments
- The handler **must** always emit either `OK\r\n` or `ERROR ...\r\n` as the
  final output, or the host will time out waiting for a terminator
- The `dfu` command is a special case — the device reboots immediately, so no
  response is sent (the host detects the disconnection)

---

## Command Reference

All planned shell commands for the AttentioLight-1. Commands marked as
**Planned** are specified but not yet implemented on one or both sides.

### `version`

Return the firmware version string.

| | |
|-|-|
| **Request** | `version\r\n` |
| **Response** | `<major>.<minor>.<patch>\r\n OK\r\n` |
| **Status** | Planned |

```
-> version\r\n
<- 1.0.0\r\n
<- OK\r\n
```

---

### `led mode <mode>`

Set the LED operating mode.

| | |
|-|-|
| **Request** | `led mode <mode>\r\n` |
| **Response** | `OK\r\n` |
| **Status** | Planned |

Known modes (preliminary, subject to change):

| Mode | Description |
|------|-------------|
| `solid` | Steady color |
| `pulse` | Breathing/pulsing effect |
| `rainbow` | Cycling through colors |

```
-> led mode pulse\r\n
<- OK\r\n
```

```
-> led mode invalid\r\n
<- ERROR unknown mode\r\n
```

---

### `led color <color>`

Set the LED color.

| | |
|-|-|
| **Request** | `led color <color>\r\n` |
| **Response** | `OK\r\n` |
| **Status** | Planned |

The color argument format (named colors, hex values, etc.) is to be defined
by the firmware. Preliminary examples use named colors:

```
-> led color red\r\n
<- OK\r\n
```

---

### `led brightness <value>`

Set the LED brightness level.

| | |
|-|-|
| **Request** | `led brightness <0-100>\r\n` |
| **Response** | `OK\r\n` |
| **Status** | Planned |

The value is an integer percentage from 0 (off) to 100 (full brightness).

```
-> led brightness 80\r\n
<- OK\r\n
```

```
-> led brightness 150\r\n
<- ERROR value out of range\r\n
```

---

### `settings get <key>`

Read a device setting value.

| | |
|-|-|
| **Request** | `settings get <key>\r\n` |
| **Response** | `<value>\r\n OK\r\n` |
| **Status** | Planned |

Known setting keys (preliminary):

| Key | Description | Example Value |
|-----|-------------|---------------|
| `serial` | Device serial number | `ABC123` |
| `name` | User-assigned device name | `My Light` |

```
-> settings get serial\r\n
<- ABC123\r\n
<- OK\r\n
```

```
-> settings get nonexistent\r\n
<- ERROR unknown key\r\n
```

---

### `settings set <key> <value>`

Write a device setting value. The value is persisted to flash (EFL).

| | |
|-|-|
| **Request** | `settings set <key> <value>\r\n` |
| **Response** | `OK\r\n` |
| **Status** | Planned |

Quoted values are supported for strings containing spaces:

```
-> settings set name "My Light"\r\n
<- OK\r\n
```

Some keys may be read-only (e.g. `serial`):

```
-> settings set serial XYZ\r\n
<- ERROR key is read-only\r\n
```

---

### `dfu`

Reboot the device into DFU bootloader mode for firmware flashing.

| | |
|-|-|
| **Request** | `dfu\r\n` |
| **Response** | None (device reboots immediately) |
| **Status** | Planned |

This is a special command — the device reboots into the STM32 system
bootloader. The USB connection drops and the device re-enumerates with DFU
descriptors. The host CLI detects this as a disconnection and proceeds with
the DFU flashing flow using `dfu-libusb`.

```
-> dfu\r\n
   (device disconnects and re-enumerates in DFU mode)
```

---

## Error Responses

All errors follow the format `ERROR <message>\r\n`. The message is a
human-readable string with no fixed schema, but implementations should use
consistent wording.

### Common Error Messages

| Error Message | Cause |
|---------------|-------|
| `unknown command` | Unrecognized command name (from ChibiOS shell) |
| `missing arguments` | Required arguments not provided |
| `unknown mode` | Invalid LED mode name |
| `value out of range` | Numeric argument outside valid bounds |
| `unknown key` | Settings key not recognized |
| `key is read-only` | Attempted to write a read-only setting |

Error messages are not machine-parsed by the CLI — they are displayed to the
user as-is. The CLI identifies errors by the `ERROR` prefix only.

---

## Implementation Status

| Command | CLI (attentio) | Firmware (AL-1) |
|---------|:--------------:|:---------------:|
| `version` | Via `send` | Planned |
| `led mode` | Stub (Phase 4) | Planned |
| `led color` | Stub (Phase 4) | Planned |
| `led brightness` | Stub (Phase 4) | Planned |
| `settings get` | Stub (Phase 5) | Planned |
| `settings set` | Stub (Phase 5) | Planned |
| `dfu` | Stub (Phase 6) | Planned |

The CLI `send` and `shell` commands can already send any arbitrary command
string to the device — the command-specific CLI subcommands (`led`,
`settings`, `dfu`) add argument validation and structured output on the host
side.
