//! Boards and devices, as PlatformIO Core reports them, plus the USB-id
//! matching that turns a plugged-in device into a board id.
//!
//! Everything here is pure: the host (and `lib.rs`) does the shelling out and
//! the file reading, this module only parses JSON and matches.

use serde_json::Value;
use std::fmt;

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Board {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub mcu: String,
    pub vendor: String,
    pub frameworks: Vec<String>,
}

impl Board {
    /// `vendor · mcu`, skipping anything the board definition leaves out.
    #[allow(dead_code)]
    pub fn summary(&self) -> String {
        [self.vendor.as_str(), self.mcu.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Device {
    pub port: String,
    pub description: String,
    pub hwid: String,
}

impl Device {
    /// The USB id of this device, if Core reported one.
    pub fn usb_id(&self) -> Option<UsbId> {
        UsbId::parse(&self.hwid)
    }
}

/// A USB id as `239A:80ED`, uppercase hex, four digits per part.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UsbId {
    pub vid: String,
    pub pid: String,
}

impl UsbId {
    /// Parses `239A:80ED`, `0x239A:0x80ED`, `239a:80ed`,
    /// `USB VID:PID=303A:1001 SER=...`, …
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty()
            || raw.eq_ignore_ascii_case("n/a")
            || raw.eq_ignore_ascii_case("none")
            || raw.eq_ignore_ascii_case("null")
        {
            return None;
        }
        // PlatformIO may report `USB VID:PID=XXXX:YYYY SER=...`; extract the id.
        let candidate = if let Some(idx) = raw.to_ascii_uppercase().find("VID:PID=") {
            let rest = &raw[idx + "VID:PID=".len()..];
            rest.split_whitespace().next().unwrap_or(rest)
        } else {
            raw
        };
        let mut parts = candidate.split(':');
        let vid = parts.next()?.trim();
        let pid = parts.next()?.trim();
        Some(Self {
            vid: Self::part(vid)?,
            pid: Self::part(pid)?,
        })
    }

    pub fn from_pair(vid: &str, pid: &str) -> Option<Self> {
        Some(Self {
            vid: Self::part(vid)?,
            pid: Self::part(pid)?,
        })
    }

    fn part(raw: &str) -> Option<String> {
        let raw = raw.trim();
        let raw = raw
            .strip_prefix("0x")
            .or_else(|| raw.strip_prefix("0X"))
            .unwrap_or(raw);
        if raw.is_empty() {
            return None;
        }
        let n = u32::from_str_radix(raw, 16).ok()?;
        Some(format!("{:04X}", n))
    }
}

impl fmt::Display for UsbId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.vid, self.pid)
    }
}

/// A board an installed platform provides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardRef {
    pub id: String,
    pub platform: String,
}

/// Index of USB id → board, built from the board JSON files under
/// `<core_dir>/platforms/*/boards/*.json`.
#[derive(Default)]
pub struct HwidIndex {
    entries: Vec<(UsbId, BoardRef)>,
}

impl HwidIndex {
    /// Records the USB ids a board definition declares. Duplicate `(id, platform)`
    /// entries for the same id are ignored.
    pub fn add(&mut self, platform: &str, id: &str, definition: &str) {
        let Ok(value) = serde_json::from_str::<Value>(definition) else {
            return;
        };
        let Some(hwids) = value
            .get("build")
            .and_then(|b| b.get("hwids"))
            .and_then(|h| h.as_array())
        else {
            return;
        };
        for entry in hwids {
            let Some(arr) = entry.as_array() else { continue };
            if arr.len() < 2 {
                continue;
            }
            let vid = arr[0].as_str().unwrap_or("");
            let pid = arr[1].as_str().unwrap_or("");
            let Some(usb) = UsbId::from_pair(vid, pid) else {
                continue;
            };
            if self
                .entries
                .iter()
                .any(|(u, r)| u == &usb && r.id == id && r.platform == platform)
            {
                continue;
            }
            self.entries.push((usb, BoardRef {
                id: id.to_owned(),
                platform: platform.to_owned(),
            }));
        }
    }

    /// Every board declaring this USB id, in insertion order.
    pub fn boards_for(&self, usb: &UsbId) -> Vec<&BoardRef> {
        self.entries
            .iter()
            .filter(|(u, _)| u == usb)
            .map(|(_, r)| r)
            .collect()
    }

}

/// Extra properties of one board, read from its definition JSON file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BoardInfo {
    pub name: String,
    pub mcu: String,
    pub vendor: String,
    pub frameworks: Vec<String>,
}

/// Parses a single board definition JSON (from `~/.platformio/platforms/<p>/boards/<id>.json`).
pub fn parse_board_info(json: &str) -> Option<BoardInfo> {
    let value: Value = serde_json::from_str(json).ok()?;
    let obj = value.as_object()?;
    Some(BoardInfo {
        name: obj.get("name").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
        mcu: obj
            .get("mcu")
            .and_then(|v| v.as_str())
            .or_else(|| obj.get("build").and_then(|b| b.get("mcu")).and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_owned(),
        vendor: obj.get("vendor").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
        frameworks: obj
            .get("frameworks")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
    })
}

/// Parses `pio boards --json-output`.
pub fn parse_boards(json: &str) -> Vec<Board> {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let Some(obj) = item.as_object() else { continue };
        let id = get_str(obj, "id");
        if id.is_empty() {
            continue;
        }
        out.push(Board {
            id,
            name: get_str(obj, "name"),
            platform: get_str(obj, "platform"),
            mcu: get_str(obj, "mcu"),
            vendor: get_str(obj, "vendor"),
            frameworks: obj
                .get("frameworks")
                .and_then(|f| f.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    out
}

/// Parses `pio device list --json-output`.
pub fn parse_devices(json: &str) -> Vec<Device> {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in arr {
        let Some(obj) = item.as_object() else { continue };
        out.push(Device {
            port: get_str(obj, "port"),
            description: get_str(obj, "description"),
            hwid: get_str(obj, "hwid"),
        });
    }
    out
}

fn get_str(obj: &serde_json::Map<String, Value>, key: &str) -> String {
    obj
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOARDS_SAMPLE: &str = r#"[
      {"id":"esp32dev","name":"ESP32 Dev Module","platform":"espressif32","mcu":"ESP32","vendor":"Espressif","frameworks":["arduino","espidf"]},
      {"id":"esp32-c3-devkitm-1","name":"ESP32-C3-DevKitM-1","platform":"espressif32","mcu":"ESP32C3","vendor":"Espressif","frameworks":["arduino"]}
    ]"#;

    #[test]
    fn parses_boards() {
        let boards = parse_boards(BOARDS_SAMPLE);
        assert_eq!(boards.len(), 2);
        assert_eq!(boards[0].id, "esp32dev");
        assert_eq!(boards[0].summary(), "Espressif · ESP32");
        assert_eq!(boards[1].frameworks, vec!["arduino"]);
    }

    #[test]
    fn ignores_garbage() {
        assert!(parse_boards("not json").is_empty());
        assert!(parse_boards("42").is_empty());
    }

    const DEVICES_SAMPLE: &str = r#"[
      {"port":"/dev/cu.usbmodem1","description":"USB JTAG","hwid":"239A:80ED"},
      {"port":"/dev/cu.debug-console","description":"n/a","hwid":"n/a"}
    ]"#;

    #[test]
    fn parses_devices_and_normalises_hwid() {
        let devices = parse_devices(DEVICES_SAMPLE);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].usb_id().map(|u| u.to_string()), Some("239A:80ED".into()));
        assert!(devices[1].usb_id().is_none());
    }

    #[test]
    fn parses_vidpid_from_usb_string() {
        let d = Device {
            port: "/dev/cu.usbmodem".into(),
            description: "USB JTAG".into(),
            hwid: "USB VID:PID=303A:1001 SER=10:00:3B:AE:DB:F8 LOCATION=2-1".into(),
        };
        assert_eq!(d.usb_id().map(|u| u.to_string()), Some("303A:1001".into()));
    }

    #[test]
    fn usb_id_handles_hex_prefix_and_case() {
        assert_eq!(UsbId::parse("0x239a:0x80ed").unwrap().to_string(), "239A:80ED");
        assert_eq!(UsbId::parse("0X303A:0x1001").unwrap().to_string(), "303A:1001");
        assert!(UsbId::parse("n/a").is_none());
        assert!(UsbId::parse("no-colon").is_none());
    }

    const BOARD_DEF: &str = r#"{"build":{"hwids":[["0x239A","0x80ED"],["0x239A","0x8117"]]}}"#;

    #[test]
    fn hwid_index_matches_a_device() {
        let mut index = HwidIndex::default();
        index.add("espressif32", "adafruit_feather_esp32s2", BOARD_DEF);
        let usb = UsbId::parse("239A:80ED").unwrap();
        let hits = index.boards_for(&usb);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "adafruit_feather_esp32s2");
        assert_eq!(hits[0].platform, "espressif32");
        // a different id does not match
        let other = UsbId::parse("1234:5678").unwrap();
        assert!(index.boards_for(&other).is_empty());
    }

    #[test]
    fn hwid_index_dedupes_identical_entries() {
        let mut index = HwidIndex::default();
        index.add("espressif32", "board", BOARD_DEF);
        index.add("espressif32", "board", BOARD_DEF);
        let usb = UsbId::parse("239A:80ED").unwrap();
        assert_eq!(index.boards_for(&usb).len(), 1);
    }
}