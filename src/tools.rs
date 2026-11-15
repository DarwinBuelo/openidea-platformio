//! The rows the PlatformIO tool window lists, and the commands they run.
//! Pure data, so the window, the commands and the tests cannot drift apart.

/// A heading in the tool window.
#[allow(dead_code)]
pub struct Group {
    pub key: &'static str,
    pub label: &'static str,
    pub tools: &'static [Tool],
}

/// One row: the command it runs, the Core command line behind it, and its icon.
#[allow(dead_code)]
pub struct Tool {
    /// The command id this row runs, and its tree key.
    pub id: &'static str,
    pub label: &'static str,
    /// The Core work behind it, shown greyed after the label.
    pub detail: &'static str,
    /// An IDE icon name.
    pub icon: &'static str,
}

pub const GROUPS: &[Group] = &[
    Group {
        key: "tasks",
        label: "Tasks",
        tools: &[
            Tool { id: "build", label: "Build", detail: "pio run -e <env>", icon: "build-project" },
            Tool { id: "upload", label: "Upload", detail: "pio run -e <env> -t upload", icon: "arrow-up" },
            Tool { id: "upload-monitor", label: "Upload and Monitor", detail: "upload, then monitor", icon: "run-to-cursor" },
            Tool { id: "monitor", label: "Monitor", detail: "pio device monitor -e <env>", icon: "db-console" },
            Tool { id: "check", label: "Check", detail: "pio check -e <env>", icon: "green-checkmark" },
            Tool { id: "clean", label: "Clean", detail: "pio run -e <env> -t clean", icon: "skip-tests" },
            Tool { id: "build-check", label: "Build & Check", detail: "pio run -e <env> (verify includes)", icon: "test-passed" },
            Tool { id: "test", label: "Test", detail: "pio test -e <env>", icon: "run-coverage" },
            Tool { id: "set-port", label: "Set Port…", detail: "pio device list", icon: "db-add" },
            Tool { id: "update-index", label: "Update Code Index", detail: "pio run -t compiledb", icon: "toolwindow-find" },
        ],
    },
    Group {
        key: "boards",
        label: "Boards",
        tools: &[
            Tool { id: "add-board", label: "Add Board…", detail: "pio project init -b <board>", icon: "inline-add" },
            Tool { id: "remove-env", label: "Remove Environment", detail: "remove an [env:…] from platformio.ini", icon: "diff-remove" },
            Tool { id: "detect-board", label: "Detect Board", detail: "pio device list + board hwids", icon: "search-history" },
        ],
    },
    Group {
        key: "libraries",
        label: "Libraries",
        tools: &[
            Tool { id: "add-library", label: "Add Library…", detail: "pio pkg install -l <spec>", icon: "add-emoji" },
        ],
    },
    Group {
        key: "core",
        label: "Core",
        tools: &[
            Tool { id: "install-python", label: "Install Python 3", detail: "installs python3 if it is missing", icon: "run-anything" },
            Tool { id: "install-core", label: "Install PlatformIO Core", detail: "downloads and runs get-platformio.py", icon: "build-offline" },
            Tool { id: "core-version", label: "Show Core Version", detail: "pio --version", icon: "test-pending" },
            Tool { id: "upgrade-core", label: "Upgrade Core", detail: "pio upgrade", icon: "build-rebuild" },
            Tool { id: "run-command", label: "Run Command…", detail: "any pio command", icon: "toolwindow-run" },
            Tool { id: "terminal", label: "Terminal", detail: "open a shell with pio on PATH", icon: "toolwindow-terminal" },
        ],
    },
];

/// Every tool, in window order.
pub fn tools() -> impl Iterator<Item = &'static Tool> {
    GROUPS.iter().flat_map(|group| group.tools.iter())
}

/// The tool a tree key names, if it names one.
pub fn find(id: &str) -> Option<&'static Tool> {
    tools().find(|tool| tool.id == id)
}

/// The ids declared as commands in plugin.toml. Kept in sync by hand; the test
/// below fails if a row is added or removed without updating the manifest.
#[allow(dead_code)]
pub const COMMAND_IDS: &[&str] = &[
    "install-python",
    "install-core",
    "build", "upload", "upload-monitor", "monitor", "check", "clean",
    "add-board", "remove-env", "detect-board", "add-library",
    "core-version", "upgrade-core", "run-command", "terminal", "update-index", "test", "set-port", "build-check",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn keys_are_unique() {
        let mut seen = BTreeSet::new();
        for group in GROUPS {
            assert!(seen.insert(group.key), "duplicate group key {}", group.key);
            for tool in group.tools {
                assert!(seen.insert(tool.id), "duplicate tool key {}", tool.id);
            }
        }
    }

    #[test]
    fn every_tool_is_findable() {
        for tool in tools() {
            assert_eq!(find(tool.id).map(|t| t.label), Some(tool.label));
        }
        assert!(find("no-such-tool").is_none());
    }

    #[test]
    fn row_ids_match_the_declared_commands() {
        let row_ids: BTreeSet<&str> = tools().map(|t| t.id).collect();
        let cmd_ids: BTreeSet<&str> = COMMAND_IDS.iter().copied().collect();
        assert_eq!(row_ids, cmd_ids);
    }
}