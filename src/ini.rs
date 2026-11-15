//! Reading `platformio.ini`: the environments it defines, and the one the task
//! rows act on.
//!
//! Only the keys the tool window shows are read. The file stays PlatformIO's:
//! nothing here writes to it, and an unknown key or a multi-line value (a
//! `build_flags` block, say) is simply skipped.

/// One `[env:<name>]` section.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Env {
    pub name: String,
    pub platform: Option<String>,
    pub board: Option<String>,
    pub framework: Option<String>,
    pub monitor_speed: Option<String>,
}

impl Env {
    /// `espressif32 · arduino`, skipping the keys the section leaves out.
    #[allow(dead_code)]
    pub fn summary(&self) -> String {
        let parts: Vec<&str> = [self.platform.as_deref(), self.framework.as_deref()]
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty())
            .collect();
        parts.join(" · ")
    }

    /// The board id, or the environment name when the section sets no board
    /// (PlatformIO's own default).
    #[allow(dead_code)]
    pub fn board(&self) -> &str {
        self.board.as_deref().filter(|b| !b.is_empty()).unwrap_or(&self.name)
    }
}

/// The environments of one project and its `default_envs`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Project {
    pub envs: Vec<Env>,
    pub default_envs: Vec<String>,
}

impl Project {
    /// The environment a saved choice names, else the first of `default_envs`,
    /// else the first section. `None` when the file defines none.
    pub fn active(&self, saved: Option<&str>) -> Option<&Env> {
        let named = saved
            .into_iter()
            .chain(self.default_envs.iter().map(String::as_str))
            .find_map(|name| self.envs.iter().find(|env| env.name == name));
        named.or_else(|| self.envs.first())
    }

    /// The index of [`Self::active`], for the tree key and the toolbar.
    #[allow(dead_code)]
    pub fn active_index(&self, saved: Option<&str>) -> Option<usize> {
        let active = self.active(saved)?;
        self.envs.iter().position(|env| env.name == active.name)
    }
}

/// Removes one `[env:<name>]` section (and its keys) from the ini text, and
/// drops `name` from `default_envs`. Returns the rewritten text.
pub fn remove_env(text: &str, env: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut section: Option<String> = None;
    let mut skipping = false;
    for line in text.split('\n') {
        let t = line.trim();
        if let Some(name) = section_header(t) {
            skipping = name
                .strip_prefix("env:")
                .map(|n| n.trim() == env)
                .unwrap_or(false);
            section = Some(name.to_string());
            if skipping {
                continue;
            }
            out.push(line.to_string());
            continue;
        }
        if skipping {
            continue;
        }
        if section.as_deref() == Some("platformio") {
            if let Some((key, value)) = t.split_once('=') {
                if key.trim() == "default_envs" {
                    let kept: Vec<&str> = value
                        .split(',')
                        .map(str::trim)
                        .filter(|x| !x.is_empty() && *x != env)
                        .collect();
                    if kept.is_empty() {
                        continue;
                    }
                    let leading = line.len() - line.trim_start().len();
                    let prefix = &line[..leading];
                    out.push(format!("{}default_envs = {}", prefix, kept.join(", ")));
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    out.join("\n")
}

/// Reads the file's text into the sections the tool window lists.
pub fn parse(text: &str) -> Project {
    let mut project = Project::default();
    let mut section = Section::None;
    for (number, line) in text.split('\n').enumerate() {
        let line = if number == 0 { line.trim_start_matches('\u{feff}') } else { line };
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = section_header(line) {
            section = match name.strip_prefix("env:") {
                Some(env) => {
                    project.envs.push(Env { name: env.trim().to_owned(), ..Env::default() });
                    Section::Env(project.envs.len() - 1)
                }
                None if name.trim() == "platformio" => Section::PlatformIo,
                // `[env]` is PlatformIO's shared default block; the keys shown
                // here are per environment, so it is skipped like any other.
                None => Section::Other,
            };
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        match section {
            Section::Env(index) => {
                let env = &mut project.envs[index];
                // The first assignment wins, as PlatformIO merges a later
                // section over an earlier one only within the same name.
                let slot = match key {
                    "platform" => &mut env.platform,
                    "board" => &mut env.board,
                    "framework" => &mut env.framework,
                    "monitor_speed" => &mut env.monitor_speed,
                    _ => continue,
                };
                if slot.is_none() && !value.is_empty() {
                    *slot = Some(value.to_owned());
                }
            }
            Section::PlatformIo if key == "default_envs" => {
                project.default_envs = value
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            _ => {}
        }
    }
    project
}

enum Section {
    None,
    PlatformIo,
    Env(usize),
    Other,
}

/// `[env:name]` or `[platformio]`, without the brackets.
fn section_header(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('[')?;
    Some(rest.split_once(']')?.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const C3BRIDGE: &str = r#"
; PlatformIO Project Configuration File
;
; Please visit documentation for the other options and examples
; https://docs.platformio.org/page/projectconf.html

[env:esp32-c3-devkitm-1]
platform = espressif32
board = esp32-c3-devkitm-1
framework = arduino
monitor_speed = 115200
build_flags =
    -D ARDUINO_USB_MODE=1
    -D ARDUINO_USB_CDC_ON_BOOT=1
"#;

    #[test]
    fn reads_the_environment_of_a_real_project_file() {
        let project = parse(C3BRIDGE);
        assert_eq!(
            project.envs,
            vec![Env {
                name: "esp32-c3-devkitm-1".into(),
                platform: Some("espressif32".into()),
                board: Some("esp32-c3-devkitm-1".into()),
                framework: Some("arduino".into()),
                monitor_speed: Some("115200".into()),
            }]
        );
        // The continuation lines of build_flags are not keys of an environment.
        assert_eq!(project.active(None).map(|env| env.name.as_str()), Some("esp32-c3-devkitm-1"));
        assert_eq!(project.envs[0].summary(), "espressif32 · arduino");
    }

    #[test]
    fn a_file_without_environments_has_no_active_one() {
        let project = parse("; nothing yet\n[platformio]\ndefault_envs = a\n");
        assert!(project.envs.is_empty());
        assert!(project.active(None).is_none());
        assert_eq!(project.default_envs, ["a"]);
    }

    #[test]
    fn default_envs_and_a_saved_choice_pick_the_environment() {
        let project = parse(
            "[platformio]\ndefault_envs = s3, c3\n\n[env:c3]\nboard = esp32-c3-devkitm-1\n\n[env:s3]\nboard = esp32-s3-devkitc-1\n",
        );
        assert_eq!(project.active(None).map(|e| e.name.as_str()), Some("s3"));
        assert_eq!(project.active_index(None), Some(1));
        // A saved choice wins over default_envs, and a stale one is ignored.
        assert_eq!(project.active(Some("c3")).map(|e| e.name.as_str()), Some("c3"));
        assert_eq!(project.active_index(Some("c3")), Some(0));
        assert_eq!(project.active(Some("gone")).map(|e| e.name.as_str()), Some("s3"));
    }

    #[test]
    fn env_names_with_odd_characters_still_read_and_the_first_key_wins() {
        let project = parse("[env:My Board]\r\nboard = one\r\nboard = two\r\nboard= three\r\n");
        assert_eq!(project.envs[0].name, "My Board");
        assert_eq!(project.envs[0].board.as_deref(), Some("one"));
        assert_eq!(project.envs[0].board(), "one");
    }

    #[test]
    fn remove_env_drops_section_and_default() {
        let text = "[platformio]\ndefault_envs = a, b\n\n[env:a]\nboard = x\n\n[env:b]\nboard = y\n";
        let out = remove_env(text, "a");
        assert!(!out.contains("[env:a]"));
        assert!(out.contains("[env:b]"));
        assert!(out.contains("default_envs = b"));
        assert!(!out.contains("default_envs = a, b"));
    }

    #[test]
    fn an_environment_without_a_board_falls_back_to_its_name() {
        let project = parse("[env:esp32dev]\nplatform = espressif32\nframework =\n");
        let env = &project.envs[0];
        assert_eq!(env.board(), "esp32dev");
        assert_eq!(env.framework, None, "an empty value is not a setting");
        assert_eq!(env.summary(), "espressif32");
    }
}

