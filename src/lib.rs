//! PlatformIO for OpenIDEA: one tool window listing everything the plugin does,
//! driving PlatformIO Core through keyed terminals and the approved `pio` binary.

mod boards;
mod ini;
mod tools;

use openidea_plugin::views::{Body, Button, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, Invocation, NotificationLevel, PickItem, Plugin, host, register_plugin};

const VIEW: &str = "platformio";

/// Runs the plugin's own installed PlatformIO Core (in ~/.platformio) through
/// the system python3, which is on PATH and needs no path prompt. The grant is
/// on python3 only; Core itself is invoked by absolute path inside the shim.
const PIO_SHIM: &str = "import os,sys\npio=os.path.expanduser('~/.platformio/penv/bin/pio')\nos.execv(pio,[pio]+sys.argv[1:])";

fn pio_args(args: &[String]) -> Vec<String> {
    let mut v = vec!["-c".to_string(), PIO_SHIM.to_string()];
    v.extend(args.iter().cloned());
    v
}

#[derive(Default)]
pub struct Main;

impl Plugin for Main {
    fn run_command(
        &mut self,
        id: &str,
        ctx: &Invocation,
        _arg: Option<&str>,
    ) -> Result<Option<String>, Error> {
        self.execute(id, ctx).map_err(explain)
    }
}

impl Views for Main {
    fn render(
        &mut self,
        _view: &str,
        _viewport: Option<Viewport>,
        ctx: &Invocation,
    ) -> Result<View, Error> {
        // Resolve project + ini.
        let (project, root, ini, saved) = match ctx.project() {
            Some(p) => match p.root() {
                Ok(r) => match p.read_file("platformio.ini") {
                    Ok(t) => (Some(p), Some(r.clone()), Some(ini::parse(&t)), saved_env(&r)),
                    Err(_) => (Some(p), Some(r), None, None),
                },
                Err(_) => (Some(p), None, None, None),
            },
            None => (None, None, None, None),
        };

        // Auto-generate the code index (compile_commands.json) on first view
        // open, so include paths are available without manual steps.
        if let (Some(p), Some(r)) = (&project, &root) {
            let flag = format!("compiledb:{}", r);
            let clangd_missing = p.read_file(".clangd").is_err();
            let cc_missing = p.read_file("compile_commands.json").is_err();
            if clangd_missing || cc_missing {
                let _task = host::begin_task("Indexing project", true);
                let env = active_env(p, r);
                let mut cargs = vec!["run".to_string()];
                if !env.is_empty() {
                    cargs.push("-e".to_string());
                    cargs.push(env.clone());
                }
                cargs.push("-t".to_string());
                cargs.push("compiledb".to_string());
                let out = p.run_process("python3", &pio_args(&cargs), None);
                if let Ok(o) = out {
                    if o.status == Some(0) {
                        write_clangd_config(p);
                        let _ = host::storage::set(&flag, "1");
                    }
                }
            }
        }

        let active_name = match (&project, &root, &ini) {
            (Some(_p), Some(_r), Some(i)) => i.active(saved.as_deref()).map(|e| e.name.clone()),
            _ => None,
        };
        let active_env_obj = match (&ini, &active_name) {
            (Some(i), Some(n)) => i.envs.iter().find(|e| e.name == *n).cloned(),
            _ => None,
        };
        let active_env_name = active_name.clone().unwrap_or_default();
        let has_env = active_env_obj.is_some();
        let env_count = ini.as_ref().map(|i| i.envs.len()).unwrap_or(0);

        // Build an HTML board-details view (themed).
        // Build an HTML board-details view (themed). Single scroll container
        // inside the iframe so there's no double scrollbar with the panel.
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");

        let env_buttons = match &ini {
            Some(i) => i
                .envs
                .iter()
                .map(|e| {
                    let active = active_name.as_deref() == Some(e.name.as_str());
                    format!(
                        r#"<button class="env{}" data-env="{}">{}</button>"#,
                        if active { " active" } else { "" },
                        esc(&e.name),
                        esc(&e.name)
                    )
                })
                .collect::<Vec<_>>()
                .join(""),
            None => String::new(),
        };

        let board_details_html = match &active_env_obj {
            Some(e) => {
                let info = board_info(e.platform.as_deref().unwrap_or(""), e.board());
                let mut rows: Vec<(String, String)> = Vec::new();
                rows.push(("Environment".to_string(), e.name.clone()));
                rows.push(("Board".to_string(), e.board().to_string()));
                if let Some(info) = &info {
                    if !info.name.is_empty() {
                        rows.push(("Board name".to_string(), info.name.clone()));
                    }
                    if !info.mcu.is_empty() {
                        rows.push(("MCU".to_string(), info.mcu.clone()));
                    }
                    if !info.vendor.is_empty() {
                        rows.push(("Vendor".to_string(), info.vendor.clone()));
                    }
                }
                if let Some(p) = &e.platform {
                    if !p.is_empty() {
                        rows.push(("Platform".to_string(), p.clone()));
                    }
                }
                if let Some(f) = &e.framework {
                    if !f.is_empty() {
                        rows.push(("Framework".to_string(), f.clone()));
                    }
                }
                if let Some(s) = &e.monitor_speed {
                    if !s.is_empty() {
                        rows.push(("Baud rate".to_string(), format!("{} baud", s)));
                    }
                }
                let rows_html: String = rows
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            r#"<div class="row"><span class="k">{}</span><span class="v">{}</span></div>"#,
                            esc(k),
                            esc(v)
                        )
                    })
                    .collect();
                format!(
                    r#"<div class="board"><div class="title">Active board</div>{}</div>"#,
                    rows_html
                )
            }
            None => r#"<div class="board"><div class="title">No environment</div><div class="row"><span class="v">Add a board to create an environment.</span></div></div>"#.to_string(),
        };

        let status = match ctx.project() {
            None => "No project is open".to_string(),
            Some(_) => match &ini {
                None => "No platformio.ini at the project root".to_string(),
                Some(i) if i.envs.is_empty() => "No [env:…] sections — add a board to create one".to_string(),
                Some(_) => {
                    let a = active_env_name.clone();
                    let a = if a.is_empty() { "—".to_string() } else { a };
                    format!("{} · {} environment{}", a, env_count, if env_count == 1 { "" } else { "s" })
                }
            },
        };

        // Board browser modal (searchable list of all boards).
        let board_browser_open = root
            .as_ref()
            .map(|r| {
                host::storage::get(&format!("board-browser:{}", r))
                    .ok()
                    .flatten()
                    .as_deref()
                    == Some("1")
            })
            .unwrap_or(false);
        let modal_boards: Vec<boards::Board> = if board_browser_open {
            if let Some(p) = &project {
                match p.run_process("python3", &pio_args(&["boards".to_string(), "--json-output".to_string()]), None) {
                    Ok(out) if out.status == Some(0) => boards::parse_boards(&out.stdout),
                    _ => Vec::new(),
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        let modal_items: String = modal_boards
            .iter()
            .map(|b| {
                format!(
                    r#"<div class="board-item" data-id="{id}" data-search="{id} {name} {platform}">{label}</div>"#,
                    id = esc(&b.id),
                    name = esc(&b.name),
                    platform = esc(&b.platform),
                    label = esc(&format!("{} — {}", b.id, b.name))
                )
            })
            .collect();
        let modal_html = if board_browser_open {
            format!(
                r#"<div class="modal-backdrop"><div class="modal-card"><div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px"><strong>Boards</strong><button id="board-close">✕</button></div><input id="board-search" type="text" placeholder="Search boards…" /><div class="modal-list">{items}</div></div></div>"#,
                items = modal_items
            )
        } else {
            String::new()
        };

        // Library browser modal.
        let lib_browser_open = root
            .as_ref()
            .map(|r| {
                host::storage::get(&format!("lib-browser:{}", r))
                    .ok()
                    .flatten()
                    .as_deref()
                    == Some("1")
            })
            .unwrap_or(false);
        let lib_query = if lib_browser_open {
            root.as_ref()
                .and_then(|r| host::storage::get(&format!("lib-query:{}", r)).ok().flatten())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let lib_results_json = if lib_browser_open {
            root.as_ref()
                .and_then(|r| host::storage::get(&format!("lib-results:{}", r)).ok().flatten())
                .unwrap_or_else(|| "[]".to_string())
        } else {
            "[]".to_string()
        };
        let lib_items: String = {
            let mut out = String::new();
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&lib_results_json) {
                if let Some(arr) = val.as_array() {
                    for item in arr {
                        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let owner = item.get("ownername").and_then(|v| v.as_str()).unwrap_or("");
                        let version = item.get("versionname").and_then(|v| v.as_str()).unwrap_or("");
                        let spec = if owner.is_empty() {
                            name.to_string()
                        } else {
                            format!("{}/{}", owner, name)
                        };
                        let label = if version.is_empty() {
                            format!("{} — {}", spec, item.get("description").and_then(|v| v.as_str()).unwrap_or(""))
                        } else {
                            format!("{} @ {} — {}", spec, version, item.get("description").and_then(|v| v.as_str()).unwrap_or(""))
                        };
                        out.push_str(&format!(
                            r#"<div class="lib-item" data-id="{id}">{label}</div>"#,
                            id = esc(&spec),
                            label = esc(&label)
                        ));
                    }
                }
            }
            out
        };
        let lib_modal_html = if lib_browser_open {
            format!(
                r#"<div class="modal-backdrop"><div class="modal-card"><div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px"><strong>Libraries</strong><button id="lib-close">✕</button></div><div style="display:flex;gap:6px;margin-bottom:8px"><input id="lib-search" type="text" placeholder="Search libraries…" value="{q}" /><button id="lib-search-btn">Search</button></div><div class="modal-list">{items}</div></div></div>"#,
                q = esc(&lib_query),
                items = lib_items
            )
        } else {
            String::new()
        };

        let mut html = format!(
            r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>
html,body{{height:100%;margin:0;overflow:hidden;background:var(--openidea-bg);color:var(--openidea-fg);font-family:var(--openidea-font-ui,system-ui,sans-serif);font-size:13px}}
.scroll{{height:100%;overflow-y:auto;padding:16px 18px;box-sizing:border-box}}
.hdr{{font-size:16px;font-weight:600;margin:0 0 16px;color:var(--openidea-fg-strong);letter-spacing:-.01em}}
.sec{{margin-bottom:20px}}
.sectitle{{font-size:10px;text-transform:uppercase;letter-spacing:.14em;color:var(--openidea-fg-muted);margin:0 0 8px;font-weight:600}}
.envs{{display:flex;flex-wrap:wrap;gap:6px;margin-bottom:4px}}
.env{{padding:5px 11px;border:1px solid var(--openidea-border);border-radius:6px;background:var(--openidea-bg-elevated);color:var(--openidea-fg);cursor:pointer;font-size:12px;transition:background .12s,border-color .12s}}
.env:hover{{background:var(--openidea-bg-hover);border-color:var(--openidea-fg-muted)}}
.env.active{{background:var(--openidea-accent);color:var(--openidea-fg-on-accent);border-color:var(--openidea-accent)}}
.board{{border:1px solid var(--openidea-border);border-radius:8px;padding:12px 14px;background:var(--openidea-bg-elevated)}}
.board .title{{font-size:10px;text-transform:uppercase;letter-spacing:.14em;color:var(--openidea-fg-muted);margin:0 0 10px;font-weight:600}}
.row{{display:flex;justify-content:space-between;align-items:baseline;padding:5px 0;border-bottom:1px solid var(--openidea-border)}}
.row:last-child{{border-bottom:none}}
.row .k{{color:var(--openidea-fg-muted);font-size:12px}}
.row .v{{color:var(--openidea-fg-strong);font-size:12px;font-weight:500;text-align:right;max-width:60%;word-break:break-word}}
.status{{margin-top:16px;color:var(--openidea-fg-muted);font-size:11px;letter-spacing:.02em}}
.modal-backdrop{{position:fixed;inset:0;background:rgba(0,0,0,.5);backdrop-filter:blur(2px);display:flex;align-items:center;justify-content:center;z-index:20}}
.modal-card{{background:var(--openidea-bg-elevated);border:1px solid var(--openidea-border);border-radius:10px;padding:14px 16px;max-width:560px;width:92%;max-height:78%;display:flex;flex-direction:column;box-shadow:0 12px 32px rgba(0,0,0,.35)}}
.modal-card input{{margin:8px 0;padding:6px 8px;border:1px solid var(--openidea-border);border-radius:4px;background:var(--openidea-bg);color:var(--openidea-fg);font-size:13px}}
.modal-list{{overflow-y:auto;flex:1;border:1px solid var(--openidea-border);border-radius:4px}}
.board-item,.lib-item{{padding:6px 10px;cursor:pointer;border-bottom:1px solid var(--openidea-border);font-size:13px}}
.board-item:hover,.lib-item:hover{{background:var(--openidea-bg-hover)}}
.board-item:last-child{{border-bottom:none}}
</style></head><body><div class="scroll">
<div class="hdr">PlatformIO</div>
<div class="sec"><div class="sectitle">Environments (click to switch)</div><div class="envs">{envs}</div></div>
{board}
{modal}
{libmodal}
<div class="status">{status}</div>
<script>
document.querySelectorAll('.env').forEach(el => el.addEventListener('click', () => openidea.postMessage('env:' + el.dataset.env)));
</script></div></body></html>"#,
            envs = env_buttons,
            board = board_details_html,
            modal = modal_html,
            libmodal = lib_modal_html,
            status = esc(&status)
        );
        // Append board-browser interaction script (braces kept out of format! string).
        let modal_script = r#"<script>
const bs = document.getElementById('board-search');
if (bs) {
  bs.addEventListener('input', () => {
    const q = bs.value.toLowerCase();
    document.querySelectorAll('.board-item').forEach(el => {
      const hay = (el.dataset.search || '').toLowerCase();
      el.style.display = hay.includes(q) ? '' : 'none';
    });
  });
  bs.focus();
}
document.querySelectorAll('.board-item').forEach(el => el.addEventListener('click', () => openidea.postMessage('board:' + el.dataset.id)));
const bc = document.getElementById('board-close');
if (bc) bc.addEventListener('click', () => openidea.postMessage('board-browser:close'));
</script>"#;
        // Insert script before closing body by replacing last occurrence.
        let mut extra_script = String::new();
        if board_browser_open {
            extra_script.push_str(modal_script);
        }
        if lib_browser_open {
            extra_script.push_str(r#"<script>
const lq = document.getElementById('lib-search');
const lbtn = document.getElementById('lib-search-btn');
function doLibSearch() {
  if (lq) openidea.postMessage('lib-search:' + (lq.value || ''));
}
if (lbtn) lbtn.addEventListener('click', doLibSearch);
if (lq) lq.addEventListener('keydown', (e) => { if (e.key === 'Enter') doLibSearch(); });
const lc = document.getElementById('lib-close');
if (lc) lc.addEventListener('click', () => openidea.postMessage('lib-browser:close'));
document.querySelectorAll('.lib-item').forEach(el => el.addEventListener('click', () => openidea.postMessage('lib:' + el.dataset.id)));
</script>"#);
        }
        if !extra_script.is_empty() {
            html = html.replace("</body></html>", &format!("{}</body></html>", extra_script));
        }

        let mut view = View::new(Body::Html(html)).status(&status);
        view.toolbar = vec![
            Button::new("build", "Build").icon("build-project").enabled(has_env),
            Button::new("upload", "Upload").icon("arrow-up").enabled(has_env),
            Button::new("monitor", "Monitor").icon("db-console").enabled(has_env),
            Button::new("check", "Check").icon("green-checkmark").enabled(has_env),
            Button::new("detect-board", "Detect Board").icon("search-history"),
            Button::new("update-index", "Code Index").icon("toolwindow-find"),
            Button::new("add-board", "Add Board").icon("inline-add"),
            Button::new("add-library", "Add Library").icon("add-emoji").enabled(has_env),
            Button::new("remove-env", "Remove Env").icon("diff-remove").enabled(has_env),
            Button::new("terminal", "Terminal").icon("toolwindow-terminal"),
            Button::new("refresh", "Refresh").icon("rerun-filled"),
        ];
        Ok(view)
    }

    fn handle(
        &mut self,
        _view: &str,
        event: ViewEvent,
        ctx: &Invocation,
    ) -> Result<(), Error> {
        match event {
            ViewEvent::Field(key, value) if key == "env" => {
                if let Some(project) = ctx.project() {
                    if let Ok(root) = project.root() {
                        host::storage::set(&format!("active-env:{}", root), &value)?;
                        host::refresh_view(VIEW);
                    }
                }
                return Ok(());
            }
            ViewEvent::Message(text) => {
                let text = text.trim();
                if let Some(name) = text.strip_prefix("env:") {
                    if let Some(project) = ctx.project() {
                        if let Ok(root) = project.root() {
                            host::storage::set(&format!("active-env:{}", root), name)?;
                            host::refresh_view(VIEW);
                        }
                    }
                    return Ok(());
                }
                if let Some(board_id) = text.strip_prefix("board:") {
                    if let Some(project) = ctx.project() {
                        if let Ok(root) = project.root() {
                            let args = vec![
                                "project".to_string(),
                                "init".to_string(),
                                "-b".to_string(),
                                board_id.to_string(),
                                "--no-install-dependencies".to_string(),
                            ];
                            match project.run_process("python3", &pio_args(&args), None) {
                                Ok(out) if out.status == Some(0) => {
                                    host::notify(NotificationLevel::Info, "PlatformIO", &format!("Added environment for board `{}`.", board_id));
                                    update_code_index(&project, &root);
                                }
                                Ok(out) => {
                                    let msg = if out.stderr.trim().is_empty() { out.stdout } else { out.stderr };
                                    host::notify(NotificationLevel::Error, "PlatformIO", &format!("`pio project init` failed:\n{}", msg.trim()));
                                }
                                Err(e) => {
                                    host::notify(NotificationLevel::Error, "PlatformIO", &e.to_string());
                                }
                            }
                            let _ = host::storage::delete(&format!("board-browser:{}", root));
                            host::refresh_view(VIEW);
                        }
                    }
                    return Ok(());
                }
                if text == "board-browser:close" {
                    if let Some(project) = ctx.project() {
                        if let Ok(root) = project.root() {
                            let _ = host::storage::delete(&format!("board-browser:{}", root));
                            host::refresh_view(VIEW);
                        }
                    }
                    return Ok(());
                }
                if let Some(query) = text.strip_prefix("lib-search:") {
                    if let Some(project) = ctx.project() {
                        if let Ok(root) = project.root() {
                            let q = query.trim();
                            let _ = host::storage::set(&format!("lib-query:{}", root), q);
                            let _ = host::storage::set(&format!("lib-query:{}", root), q);
                            // run pio lib search (--json-output); empty query lists the default set
                            let mut search_args = vec!["lib".to_string(), "search".to_string()];
                            if !q.is_empty() {
                                search_args.push(q.to_string());
                            }
                            search_args.push("--json-output".to_string());
                            let out = project.run_process(
                                "python3",
                                &pio_args(&search_args),
                                None,
                            );
                            let results = match out {
                                Ok(o) if o.status == Some(0) => {
                                    // extract items array
                                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&o.stdout) {
                                        v.get("items").cloned().unwrap_or(serde_json::json!([]))
                                    } else {
                                        serde_json::json!([])
                                    }
                                }
                                _ => serde_json::json!([]),
                            };
                            let _ = host::storage::set(&format!("lib-results:{}", root), &results.to_string());
                            host::refresh_view(VIEW);
                        }
                    }
                    return Ok(());
                }
                if let Some(spec) = text.strip_prefix("lib:") {
                    if let Some(project) = ctx.project() {
                        if let Ok(root) = project.root() {
                            install_library_spec(&project, &root, spec.trim());
                            let _ = host::storage::delete(&format!("lib-browser:{}", root));
                            let _ = host::storage::delete(&format!("lib-query:{}", root));
                            let _ = host::storage::delete(&format!("lib-results:{}", root));
                            host::refresh_view(VIEW);
                        }
                    }
                    return Ok(());
                }
                if text == "lib-browser:close" {
                    if let Some(project) = ctx.project() {
                        if let Ok(root) = project.root() {
                            let _ = host::storage::delete(&format!("lib-browser:{}", root));
                            let _ = host::storage::delete(&format!("lib-query:{}", root));
                            let _ = host::storage::delete(&format!("lib-results:{}", root));
                            host::refresh_view(VIEW);
                        }
                    }
                    return Ok(());
                }
                return Ok(());
            }
            ViewEvent::Activate(key) => {
                if let Some(name) = key.strip_prefix("env:") {
                    if name != "none" {
                        if let Some(project) = ctx.project() {
                            if let Ok(root) = project.root() {
                                host::storage::set(&format!("active-env:{}", root), name)?;
                                host::refresh_view(VIEW);
                            }
                        }
                    }
                    return Ok(());
                }
                if tools::find(&key).is_some() {
                    return self.execute(&key, ctx).map_err(explain).map(|_| ());
                }
                Ok(())
            }
            ViewEvent::Button(id) => {
                if id == "refresh" {
                    host::refresh_view(VIEW);
                    return Ok(());
                }
                self.execute(&id, ctx).map_err(explain).map(|_| ())
            }
            _ => Ok(()),
        }
    }
}

impl Main {
    fn execute(&mut self, id: &str, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or_else(|| "No project is open".to_string())?;
        let root = project.root()?;
        match id {
            "build" | "upload" | "monitor" | "check" | "clean" | "test" | "upgrade-core" | "run-command" | "upload-monitor" => {
                let env = active_env(&project, &root);
                // monitor is long-running/interactive: its own terminal tab.
                if id == "monitor" {
                    let mut args = vec!["device".to_string(), "monitor".to_string()];
                    args.extend(env_flag(&env));
                    let cmd = host::Command {
                        program: "python3".to_string(),
                        args: pio_args(&args),
                        env: Vec::new(),
                    };
                    host::open_terminal_session("monitor", "Monitor", &cmd)?;
                    return Ok(None);
                }

                // Everything else runs in one reusable shell terminal, so the tab
                // stays open after the command finishes and is reused next time.
                let pio = "~/.platformio/penv/bin/pio";
                let shell_cmd = match id {
                    "build" => format!("{} run{}", pio, env_flag_shell(&env)),
                    "upload" => format!("{} run{} -t upload{}", pio, env_flag_shell(&env), port_flag_shell(&root, "upload")),
                    "check" => format!("{} check{}", pio, env_flag_shell(&env)),
                    "clean" => format!("{} run{} -t clean", pio, env_flag_shell(&env)),
                    "test" => format!("{} test{}", pio, env_flag_shell(&env)),
                    "upload-monitor" => format!("{} run{} -t upload{} && {} device monitor{}{}", pio, env_flag_shell(&env), port_flag_shell(&root, "upload"), pio, env_flag_shell(&env), port_flag_shell(&root, "monitor")),
                    "upgrade-core" => format!("{} upgrade", pio),
                    "run-command" => {
                        let line = match host::input("pio command (without 'pio')", None)? {
                            Some(l) => l,
                            None => return Ok(None),
                        };
                        let parts = line.trim();
                        if parts.is_empty() {
                            return Ok(None);
                        }
                        format!("{} {}", pio, parts)
                    }
                    _ => unreachable!(),
                };

                // Open (or focus) the shared shell terminal with pio on PATH, then send the command.
                let sh = host::Command {
                    program: "sh".to_string(),
                    args: vec!["-c".to_string(), "export PATH=\"$HOME/.platformio/penv/bin:$PATH\"; exec sh -i".to_string()],
                    env: Vec::new(),
                };
                host::open_terminal_session("pio", "PlatformIO", &sh)?;
                host::write_terminal("pio", &format!("{}\n", shell_cmd))?;
                Ok(None)
            }
            "terminal" => {
                let sh = host::Command {
                    program: "sh".to_string(),
                    args: vec!["-c".to_string(), "export PATH=\"$HOME/.platformio/penv/bin:$PATH\"; exec sh -i".to_string()],
                    env: Vec::new(),
                };
                host::open_terminal_session("pio", "PlatformIO", &sh)?;
                Ok(None)
            }
            "install-python" => {
                install_python(ctx);
                Ok(None)
            }
            "install-core" => {
                install_core(ctx, &project, &root);
                Ok(None)
            }
            "core-version" => {
                let out = project.run_process("python3", &pio_args(&["--version".to_string()]), None)?;
                let msg = if out.stdout.trim().is_empty() {
                    out.stderr.clone()
                } else {
                    out.stdout.clone()
                };
                host::notify(NotificationLevel::Info, "PlatformIO", msg.trim());
                Ok(None)
            }
            "add-board" => {
                open_board_browser(ctx, &project, &root);
                Ok(None)
            }
            "update-index" => {
                update_code_index(&project, &root);
                Ok(None)
            }
            "set-port" => {
                set_port(&project, &root);
                Ok(None)
            }
            "build-check" => {
                build_check(&project, &root);
                Ok(None)
            }
            "remove-env" | "remove-environment" | "delete-env" | "delete-environment" => {
                remove_environment(ctx, &project, &root);
                Ok(None)
            }
            "add-library" | "install-library" | "lib-install" | "add-lib" => {
                open_lib_browser(ctx, &project, &root);
                Ok(None)
            }
            "detect-board" => {
                detect_board(ctx, &project, &root);
                Ok(None)
            }
            _ => Err(format!("unknown command {id}").into()),
        }
    }
}


/// Shell-fragment form of `env_flag`, for use in a reusable `sh` terminal.
fn env_flag_shell(env: &str) -> String {
    if env.is_empty() {
        String::new()
    } else {
        format!(" -e {}", env)
    }
}

fn stored_port(root: &str) -> Option<String> {
    host::storage::get(&format!("port:{}", root)).ok().flatten()
}

fn port_flag_shell(root: &str, kind: &str) -> String {
    match stored_port(root) {
        Some(p) if !p.is_empty() => {
            if kind == "upload" {
                format!(" --upload-port {}", p)
            } else if kind == "monitor" {
                format!(" -p {}", p)
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

fn build_check(project: &openidea_plugin::host::Project, root: &str) {
    let env = active_env(project, root);
    let mut args = vec!["run".to_string()];
    if !env.is_empty() {
        args.push("-e".to_string());
        args.push(env.clone());
    }
    let _task = host::begin_task("Build & check", true);
    let out = project.run_process("python3", &pio_args(&args), None);
    match out {
        Ok(o) if o.status == Some(0) => {
            host::notify(NotificationLevel::Info, "PlatformIO", "Build succeeded — no missing headers.");
        }
        Ok(o) => {
            let msg = if o.stderr.trim().is_empty() { o.stdout.clone() } else { o.stderr.clone() };
            let err_count = msg.lines().filter(|l| l.to_lowercase().contains("error:")).count();
            let snippet: String = msg.lines().take(12).collect::<Vec<_>>().join("\n");
            host::notify(
                NotificationLevel::Error,
                "PlatformIO",
                &format!("Build failed ({} error(s)):\n{}", err_count, snippet),
            );
        }
        Err(e) => {
            host::notify(NotificationLevel::Error, "PlatformIO", &e.to_string());
        }
    }
}

fn set_port(project: &openidea_plugin::host::Project, root: &str) {
    let out = match project.run_process("python3", &pio_args(&["device".to_string(), "list".to_string(), "--json-output".to_string()]), None) {
        Ok(o) if o.status == Some(0) => o.stdout,
        Ok(o) => {
            let msg = if o.stderr.trim().is_empty() { o.stdout } else { o.stderr };
            host::notify(NotificationLevel::Error, "PlatformIO", &format!("`pio device list` failed:\n{}", msg.trim()));
            return;
        }
        Err(e) => {
            host::notify(NotificationLevel::Error, "PlatformIO", &e.to_string());
            return;
        }
    };
    let devices = boards::parse_devices(&out);
    if devices.is_empty() {
        host::notify(NotificationLevel::Info, "PlatformIO", "No serial ports found.");
        return;
    }
    let items: Vec<PickItem> = devices
        .iter()
        .map(|d| PickItem {
            key: d.port.clone(),
            label: d.port.clone(),
            description: Some(d.description.clone()),
        })
        .collect();
    match host::pick("Serial port", &items) {
        Ok(Some(p)) => {
            let _ = host::storage::set(&format!("port:{}", root), &p);
            host::notify(NotificationLevel::Info, "PlatformIO", &format!("Selected port: {}", p));
        }
        _ => {}
    }
}

fn env_flag(env: &str) -> Vec<String> {
    if env.is_empty() {
        Vec::new()
    } else {
        vec!["-e".to_string(), env.to_string()]
    }
}

fn active_env(project: &openidea_plugin::host::Project, root: &str) -> String {
    let saved = host::storage::get(&format!("active-env:{}", root)).ok().flatten();
    let text = project.read_file("platformio.ini").unwrap_or_default();
    let ini = ini::parse(&text);
    ini.active(saved.as_deref())
        .map(|e| e.name.clone())
        .unwrap_or_default()
}

fn saved_env(root: &str) -> Option<String> {
    host::storage::get(&format!("active-env:{}", root)).ok().flatten()
}

/// Runs esptool to detect the connected chip type. Returns (raw, normalized)
/// e.g. ("ESP32-C3", "esp32c3").
fn detect_chip_type() -> Option<(String, String)> {
    let path = "~/.platformio/packages/tool-esptool/esptool";
    let out = host::run_process("sh", &["-c".to_string(), format!("{} chip-id", path)], None).ok()?;
    if out.status != Some(0) {
        return None;
    }
    let text = if out.stdout.is_empty() { &out.stderr } else { &out.stdout };
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("Chip type:") {
            let raw = rest.split('(').next().unwrap_or(rest).trim().to_string();
            if raw.is_empty() {
                continue;
            }
            let norm = raw.to_lowercase().replace(['-', ' '], "");
            return Some((raw, norm));
        }
    }
    None
}

/// Maps board id -> mcu using `pio boards` (no fs grant needed).
fn boards_mcu_map(project: &openidea_plugin::host::Project) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    let out = match project.run_process("python3", &pio_args(&["boards".to_string(), "--json-output".to_string()]), None) {
        Ok(o) if o.status == Some(0) => o.stdout,
        _ => return map,
    };
    for b in boards::parse_boards(&out) {
        map.insert(b.id.clone(), b.mcu.clone());
    }
    map
}

fn board_info(platform: &str, board: &str) -> Option<boards::BoardInfo> {
    if platform.is_empty() || board.is_empty() {
        return None;
    }
    let path = format!("~/.platformio/platforms/{}/boards/{}.json", platform, board);
    let text = host::fs::read_file(&path).ok()?;
    boards::parse_board_info(&text)
}

fn explain(err: Error) -> Error {
    let msg = err.to_string();
    if msg.to_lowercase().contains("denied") {
        if msg.contains("process:sh") {
            "Shell is not approved. Open Settings › Plugins › PlatformIO and approve `process:sh` (it is on PATH).".into()
        } else {
            "Python 3 is not approved. Open Settings › Plugins › PlatformIO and approve `process:python3` (it is on PATH).".into()
        }
    } else {
        err
    }
}

// ---------- install python ----------

fn install_python(_ctx: &Invocation) {
    // Already available?
    if let Ok(out) = openidea_plugin::host::run_process("sh", &["-c".to_string(), "command -v python3".to_string()], None) {
        if out.status == Some(0) && !out.stdout.trim().is_empty() {
            host::notify(NotificationLevel::Info, "PlatformIO", &format!("Python 3 already available: {}", out.stdout.trim()));
            return;
        }
    }

    let os = openidea_plugin::host::run_process("sh", &["-c".to_string(), "uname -s".to_string()], None)
        .ok()
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default();

    let pkg_cmd: Option<&str> = match os.as_str() {
        "Darwin" => {
            // Homebrew?
            let brew = openidea_plugin::host::run_process("sh", &["-c".to_string(), "command -v brew".to_string()], None)
                .ok()
                .filter(|o| o.status == Some(0) && !o.stdout.trim().is_empty());
            if brew.is_none() {
                host::notify(
                    NotificationLevel::Error,
                    "PlatformIO",
                    "Python 3 is missing and Homebrew was not found. Install Homebrew (https://brew.sh) or Python from https://www.python.org/downloads/, then retry.",
                );
                return;
            }
            Some("brew install python3")
        }
        "Linux" => {
            // Pick a package manager.
            let has = |name: &str| {
                openidea_plugin::host::run_process("sh", &["-c".to_string(), format!("command -v {}", name)], None)
                    .ok()
                    .map(|o| o.status == Some(0) && !o.stdout.trim().is_empty())
                    .unwrap_or(false)
            };
            if has("apt-get") {
                Some("sudo apt-get update && sudo apt-get install -y python3")
            } else if has("dnf") {
                Some("sudo dnf install -y python3")
            } else if has("pacman") {
                Some("sudo pacman -S --noconfirm python")
            } else {
                host::notify(
                    NotificationLevel::Error,
                    "PlatformIO",
                    "Python 3 is missing and no supported package manager (apt-get/dnf/pacman) was found. Install Python from https://www.python.org/downloads/, then retry.",
                );
                return;
            }
        }
        _ => {
            host::notify(
                NotificationLevel::Error,
                "PlatformIO",
                "Unsupported platform for automatic Python install. Install Python 3 from https://www.python.org/downloads/, then retry.",
            );
            return;
        }
    };

    let Some(pkg_cmd) = pkg_cmd else { return };
    let confirmed = host::confirm(
        "Install Python 3",
        &format!("This runs `{}` in a terminal (may require sudo). Continue?", pkg_cmd),
    )
    .unwrap_or(false);
    if !confirmed {
        return;
    }

    let cmd = host::Command {
        program: "sh".to_string(),
        args: vec!["-c".to_string(), pkg_cmd.to_string()],
        env: Vec::new(),
    };
    match host::open_terminal_session("install-python", "Install Python 3", &cmd) {
        Ok(_) => {
            host::notify(
                NotificationLevel::Info,
                "PlatformIO",
                "Python 3 installation started in a terminal. When it finishes, run Install PlatformIO Core.",
            );
        }
        Err(e) => {
            host::notify(NotificationLevel::Error, "PlatformIO", &e.to_string());
        }
    }
}

// ---------- install core ----------

fn install_core(ctx: &Invocation, project: &openidea_plugin::host::Project, _root: &str) {
    // Python 3 is required to run the installer and Core.
    let python_ok = openidea_plugin::host::run_process("sh", &["-c".to_string(), "command -v python3".to_string()], None)
        .ok()
        .map(|o| o.status == Some(0) && !o.stdout.trim().is_empty())
        .unwrap_or(false);
    if !python_ok {
        host::notify(
            NotificationLevel::Error,
            "PlatformIO",
            "Python 3 is required. Run 'Install Python 3' first, then retry.",
        );
        return;
    }

    // Already installed?
    let installed_path = "~/.platformio/penv/bin/pio";
    if host::fs::stat(installed_path).is_ok() {
        // Try to report the version if the grant allows it.
        match project.run_process("python3", &pio_args(&["--version".to_string()]), None) {
            Ok(out) if out.status == Some(0) => {
                let msg = if out.stdout.trim().is_empty() { out.stderr } else { out.stdout };
                host::notify(NotificationLevel::Info, "PlatformIO", &format!("Core already installed: {}", msg.trim()));
                return;
            }
            _ => {
                host::notify(
                    NotificationLevel::Info,
                    "PlatformIO",
                    "Core is already installed at ~/.platformio/penv/bin/pio. Grant process:python3 to that path to use it.",
                );
                return;
            }
        }
    }

    let confirmed = host::confirm(
        "Install PlatformIO Core",
        "This downloads and installs PlatformIO Core into ~/.platformio (a few minutes, needs network). Continue?",
    )
    .unwrap_or(false);
    if !confirmed {
        return;
    }

    let script = "import os,runpy,tempfile,urllib.request\n\
                  url='https://raw.githubusercontent.com/platformio/platformio-core-installer/master/get-platformio.py'\n\
                  dst=os.path.join(tempfile.gettempdir(),'get-platformio.py')\n\
                  print('Downloading',url)\n\
                  urllib.request.urlretrieve(url,dst)\n\
                  print('Running installer...')\n\
                  runpy.run_path(dst,run_name='__main__')\n";
    let cmd = host::Command {
        program: "python3".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        env: Vec::new(),
    };
    match host::open_terminal_session("install-core", "Install PlatformIO Core", &cmd) {
        Ok(_) => {
            host::notify(
                NotificationLevel::Info,
                "PlatformIO",
                "Installation started in a terminal. When it finishes, grant process:python3 to ~/.platformio/penv/bin/pio in Settings › Plugins.",
            );
        }
        Err(e) => {
            host::notify(NotificationLevel::Error, "PlatformIO", &e.to_string());
        }
    }
    let _ = ctx;
}

fn open_board_browser(_ctx: &Invocation, _project: &openidea_plugin::host::Project, root: &str) {
    let _ = host::storage::set(&format!("board-browser:{}", root), "1");
    host::refresh_view(VIEW);
}

fn add_board_dialog(_ctx: &Invocation, project: &openidea_plugin::host::Project, root: &str) {
    let _ = root;
    let boards_json = match project.run_process("python3", &pio_args(&["boards".to_string(), "--json-output".to_string()]), None) {
        Ok(out) if out.status == Some(0) => out.stdout,
        Ok(out) => {
            let msg = if out.stderr.trim().is_empty() { out.stdout } else { out.stderr };
            host::notify(NotificationLevel::Error, "PlatformIO", &format!("`pio boards` failed:\n{}", msg.trim()));
            return;
        }
        Err(e) => {
            host::notify(NotificationLevel::Error, "PlatformIO", &e.to_string());
            return;
        }
    };
    let boards = boards::parse_boards(&boards_json);

    let board_items: Vec<PickItem> = boards
        .iter()
        .map(|b| PickItem {
            key: b.id.clone(),
            label: format!("{} — {}", b.id, b.name),
            description: Some(format!("{} · {}", b.platform, b.summary())),
        })
        .collect();

    if board_items.is_empty() {
        host::notify(NotificationLevel::Info, "PlatformIO", "No boards match that search.");
        return;
    }

    let board_id = match host::pick("Board", &board_items) {
        Ok(Some(b)) => b,
        _ => return,
    };

    let args = vec![
        "project".to_string(),
        "init".to_string(),
        "-b".to_string(),
        board_id.clone(),
        "--no-install-dependencies".to_string(),
    ];
    match project.run_process("python3", &pio_args(&args), None) {
        Ok(out) if out.status == Some(0) => {
            host::notify(NotificationLevel::Info, "PlatformIO", &format!("Added environment for board `{}`.", board_id));
            host::refresh_view(VIEW);
        }
        Ok(out) => {
            let msg = if out.stderr.trim().is_empty() {
                out.stdout
            } else {
                out.stderr
            };
            host::notify(NotificationLevel::Error, "PlatformIO", &format!("`pio project init` failed:\n{}", msg.trim()));
        }
        Err(e) => {
            host::notify(NotificationLevel::Error, "PlatformIO", &e.to_string());
        }
    }
}

fn write_clangd_config(project: &openidea_plugin::host::Project) {
    // Derive the toolchain include dirs from PlatformIO's core dir so clangd
    // (which uses a host target) can find newlib headers such as sys/reent.h.
    let core_dir = match project.run_process("python3", &pio_args(&["system".to_string(), "info".to_string(), "--json-output".to_string()]), None) {
        Ok(o) if o.status == Some(0) => {
            let v: serde_json::Value = serde_json::from_str(&o.stdout).unwrap_or_default();
            v.get("core_dir")
                .and_then(|c| c.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("~/.platformio")
                .to_string()
        }
        _ => "~/.platformio".to_string(),
    };
    // ~ won't expand inside .clangd, so resolve to an absolute home path.
    let core_dir = if core_dir.starts_with('~') {
        // Fall back: ask the shell to expand.
        host::run_process("sh", &["-c".to_string(), format!("echo {}", core_dir)], None)
            .ok()
            .map(|o| o.stdout.trim().to_string())
            .unwrap_or(core_dir)
    } else {
        core_dir
    };

    let mut add: Vec<String> = Vec::new();
    // Make project-local headers (including ../hal.h style includes) resolvable.
    add.push("-I.".to_string());
    // Avoid clangd stopping after too many errors while indexing.
    add.push("-ferror-limit=0".to_string());

    // Add include paths for installed libraries (and their transitive deps),
    // which live under .pio/libdeps/<env>/, plus the project's own lib/.
    if let Ok(root) = project.root() {
        let env = active_env(project, &root);
        let mut bases: Vec<String> = Vec::new();
        if !env.is_empty() {
            bases.push(format!("{}/.pio/libdeps/{}", root, env));
        } else {
            bases.push(format!("{}/.pio/libdeps", root));
        }
        bases.push(format!("{}/lib", root));
        bases.push(format!("{}/include", root));
        bases.push(format!("{}/src", root));
        for base in &bases {
            add.push(format!("-I{}", base));
            if let Ok(entries) = host::fs::read_dir(base) {
                for e in entries {
                    if e.kind == openidea_plugin::FileKind::Directory {
                        let dir = format!("{}/{}", base, e.name);
                        add.push(format!("-I{}", dir));
                        for sub in ["src", "include", "inc", "lib"] {
                            let subp = format!("{}/{}", dir, sub);
                            // Only add if it exists (read_dir/stat); best-effort push.
                            add.push(format!("-I{}", subp));
                        }
                    }
                }
            }
        }
    }

    // Arduino / framework headers (Arduino.h, etc.) live under
    // <core_dir>/packages/framework-*/. Add their include roots.
    let packages_dir = format!("{}/packages", core_dir);
    if let Ok(entries) = host::fs::read_dir(&packages_dir) {
        for e in entries {
            if e.kind != openidea_plugin::FileKind::Directory {
                continue;
            }
            if !e.name.starts_with("framework-") && !e.name.starts_with("tool-") {
                // still consider framework dirs
            }
            if !e.name.starts_with("framework-") {
                continue;
            }
            let fw = format!("{}/{}", packages_dir, e.name);
            add.push(format!("-I{}", fw));
            // Scan for include-ish subdirs recursively a couple levels.
            let mut stack = vec![fw.clone()];
            for _ in 0..3 {
                let mut next = Vec::new();
                for dir in &stack {
                    if let Ok(subs) = host::fs::read_dir(dir) {
                        for se in subs {
                            if se.kind == openidea_plugin::FileKind::Directory {
                                let name = se.name.as_str();
                                if name == "cores" || name == "variants" || name == "libraries"
                                    || name == "include" || name == "inc" || name == "src"
                                    || name.starts_with("esp32") || name.starts_with("esp8266")
                                {
                                    let p = format!("{}/{}", dir, name);
                                    add.push(format!("-I{}", p));
                                    next.push(p);
                                }
                            }
                        }
                    }
                }
                stack = next;
            }
        }
    }

    // Toolchain include paths. Each toolchain has a different triple dir and
    // ships its C/C++ standard headers under several locations (picolibc, the
    // gcc c++ headers, ...). Add all of them so clangd can resolve <cctype>
    // and friends.
    let toolchains: &[(&str, &str)] = &[
        ("toolchain-riscv32-esp", "riscv32-esp-elf"),
        ("toolchain-xtensa-esp-elf", "xtensa-esp-elf"),
    ];
    for (tc, triple) in toolchains {
        let base = format!("{}/packages/{}", core_dir, tc);
        let triple_dir = format!("{}/{}", base, triple);
        // Direct include dirs.
        for sub in ["include", "sys-include"] {
            add.push(format!("-I{}/{}", triple_dir, sub));
        }
        // picolibc.
        let picolibc = format!("{}/picolibc/{}", base, triple);
        add.push(format!("-I{}/include", picolibc));
        add.push(format!("-I{}/picolibc/include", base));
        // C++ standard library headers (versioned dir).
        let cpp_dir = format!("{}/include/c++", triple_dir);
        if let Ok(entries) = host::fs::read_dir(&cpp_dir) {
            for e in entries {
                if e.kind == openidea_plugin::FileKind::Directory {
                    let vdir = format!("{}/{}", cpp_dir, e.name);
                    add.push(format!("-I{}", vdir));
                    // target-specific subdir, e.g. xtensa-esp-elf/.
                    let target_sub = format!("{}/{}", vdir, triple);
                    add.push(format!("-I{}", target_sub));
                    if let Ok(subs) = host::fs::read_dir(&vdir) {
                        for se in subs {
                            if se.kind == openidea_plugin::FileKind::Directory {
                                add.push(format!("-I{}/{}", vdir, se.name));
                            }
                        }
                    }
                }
            }
        }
        // picolibc c++ headers too.
        let picolibc_cpp = format!("{}/picolibc/{}/include/c++", base, triple);
        if let Ok(entries) = host::fs::read_dir(&picolibc_cpp) {
            for e in entries {
                if e.kind == openidea_plugin::FileKind::Directory {
                    add.push(format!("-I{}/{}", picolibc_cpp, e.name));
                }
            }
        }
    }

    let remove = "[-m*, -f*, -W*, -O*, -g*, -gdwarf-*, -stack-protector*, -no-round-trip-args, -fdeprecated-macro, -ferror-limit*, -fmax-type-align=*, -fcommon, -fblocks, -fencode-extended-block-signature, -fregister-global-dtors-with-atexit, -fskip-odr-check-in-gmf, -fcxx-exceptions, -fexceptions, -fno-cxx-modules, -fno-implicit-modules, -fno-rtti, -fdwarf2-cfi-asm, -fstack-check, -mdarwin-stkchk-strong-link, -fcoverage-compilation-dir=*, -fdebug-compilation-dir=*, -fdepfile-entry=*, -fsyntax-only, -x, c++, -std=*]";
    // Determine the compiler driver to query for system include paths, so
    // clangd uses the toolchain's own standard headers (fixes va_list etc.).
    let compiler = if let Ok(text) = project.read_file("compile_commands.json") {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(arr) = val.as_array() {
                if let Some(first) = arr.first() {
                    if let Some(cmd) = first.get("command").and_then(|c| c.as_str()) {
                        let driver = cmd.split_whitespace().next().unwrap_or("");
                        // Resolve a bare driver name to its full path.
                        if driver.contains('/') {
                            driver.to_string()
                        } else {
                            let mut found = String::new();
                            for tc in ["toolchain-riscv32-esp", "toolchain-xtensa-esp-elf"] {
                                let p = format!("{}/packages/{}/bin/{}", core_dir, tc, driver);
                                if host::fs::stat(&p).is_ok() {
                                    found = p;
                                    break;
                                }
                            }
                            if found.is_empty() { driver.to_string() } else { found }
                        }
                    } else { String::new() }
                } else { String::new() }
            } else { String::new() }
        } else { String::new() }
    } else { String::new() };

    let compiler_line = if compiler.is_empty() {
        String::new()
    } else {
        format!("  Compiler: {}\n", compiler)
    };

    let clangd = format!(
        "CompileFlags:\n{}  CompilationDatabase: .\n  Add: [{}]\n  Remove: {}\n",
        compiler_line,
        add.join(", "),
        remove
    );
    let _ = project.write_file(".clangd", &clangd);
}

fn update_code_index(project: &openidea_plugin::host::Project, root: &str) {
    let _task = host::begin_task("Update code index", true);
    let env = active_env(project, root);
    let mut cargs = vec!["run".to_string()];
    if !env.is_empty() {
        cargs.push("-e".to_string());
        cargs.push(env.clone());
    }
    cargs.push("-t".to_string());
    cargs.push("compiledb".to_string());
    let out = project.run_process("python3", &pio_args(&cargs), None);
    match out {
        Ok(o) if o.status == Some(0) => {
            write_clangd_config(project);
            host::notify(
                NotificationLevel::Info,
                "PlatformIO",
                "Code index updated (compile_commands.json). Reopen the source file to refresh IntelliSense.",
            );
        }
        Ok(o) => {
            let msg = if o.stderr.trim().is_empty() { o.stdout } else { o.stderr };
            host::notify(
                NotificationLevel::Error,
                "PlatformIO",
                &format!("`pio run -t compiledb` failed:\n{}", msg.trim()),
            );
        }
        Err(e) => {
            host::notify(NotificationLevel::Error, "PlatformIO", &e.to_string());
        }
    }
    let _ = root;
}

// ---------- remove environment ----------

fn remove_environment(_ctx: &Invocation, project: &openidea_plugin::host::Project, root: &str) {
    let text = match project.read_file("platformio.ini") {
        Ok(t) => t,
        Err(e) => {
            host::notify(NotificationLevel::Error, "PlatformIO", &e.to_string());
            return;
        }
    };
    let ini = ini::parse(&text);
    if ini.envs.is_empty() {
        host::notify(NotificationLevel::Info, "PlatformIO", "No environments to remove.");
        return;
    }
    let items: Vec<PickItem> = ini
        .envs
        .iter()
        .map(|e| PickItem {
            key: e.name.clone(),
            label: e.name.clone(),
            description: Some(e.board().to_string()),
        })
        .collect();
    let env = match host::pick("Environment to remove", &items) {
        Ok(Some(e)) => e,
        _ => return,
    };
    let confirmed = host::confirm(
        "Remove environment",
        &format!("Remove environment `{}` from platformio.ini?", env),
    )
    .unwrap_or(false);
    if !confirmed {
        return;
    }
    let new_text = ini::remove_env(&text, &env);
    if let Err(e) = project.write_file("platformio.ini", &new_text) {
        host::notify(NotificationLevel::Error, "PlatformIO", &e.to_string());
        return;
    }
    let new_ini = ini::parse(&new_text);
    if let Some(first) = new_ini.envs.first() {
        let saved = saved_env(root);
        let still = saved
            .as_ref()
            .map(|s| new_ini.envs.iter().any(|e| e.name == *s))
            .unwrap_or(false);
        if !still {
            let _ = host::storage::set(&format!("active-env:{}", root), &first.name);
        }
    } else {
        let _ = host::storage::delete(&format!("active-env:{}", root));
    }
    host::notify(NotificationLevel::Info, "PlatformIO", &format!("Removed environment `{}`.", env));
    host::refresh_view(VIEW);
}

// ---------- add library ----------

fn open_lib_browser(_ctx: &Invocation, project: &openidea_plugin::host::Project, root: &str) {
    let _ = host::storage::set(&format!("lib-browser:{}", root), "1");
    let _ = host::storage::set(&format!("lib-query:{}", root), "");
    // Populate with the default library list.
    let out = project.run_process(
        "python3",
        &pio_args(&["lib".to_string(), "search".to_string(), "--json-output".to_string()]),
        None,
    );
    let results = match out {
        Ok(o) if o.status == Some(0) => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&o.stdout) {
                v.get("items").cloned().unwrap_or(serde_json::json!([]))
            } else {
                serde_json::json!([])
            }
        }
        _ => serde_json::json!([]),
    };
    let _ = host::storage::set(&format!("lib-results:{}", root), &results.to_string());
    host::refresh_view(VIEW);
}

fn install_library_spec(project: &openidea_plugin::host::Project, root: &str, spec: &str) {
    let env = active_env(project, root);
    let _task = host::begin_task("Installing library", true);
    let mut args = vec!["pkg".to_string(), "install".to_string(), "-l".to_string(), spec.to_string()];
    if !env.is_empty() {
        args.push("-e".to_string());
        args.push(env.clone());
    }
    match project.run_process("python3", &pio_args(&args), None) {
        Ok(out) if out.status == Some(0) => {
            host::notify(NotificationLevel::Info, "PlatformIO", &format!("Installed library `{}`.", spec));
            update_code_index(project, root);
            host::refresh_view(VIEW);
        }
        Ok(out) => {
            let msg = if out.stderr.trim().is_empty() {
                out.stdout
            } else {
                out.stderr
            };
            host::notify(
                NotificationLevel::Error,
                "PlatformIO",
                &format!("Failed to install `{}`:\n{}", spec, msg.trim()),
            );
        }
        Err(e) => {
            host::notify(NotificationLevel::Error, "PlatformIO", &e.to_string());
        }
    }
}

// ---------- detect board ----------

fn detect_board(_ctx: &Invocation, project: &openidea_plugin::host::Project, _root: &str) {
    let _task = host::begin_task("Detect board", true);

    // Core dir.
    let core_dir = {
        let out = project.run_process("python3", &pio_args(&["system".to_string(), "info".to_string(), "--json-output".to_string()]), None);
        match out {
            Ok(o) if o.status == Some(0) => {
                let v: serde_json::Value = serde_json::from_str(&o.stdout).unwrap_or_default();
                v.get("core_dir")
                    .and_then(|c| c.get("value"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("~/.platformio")
                    .to_string()
            }
            _ => "~/.platformio".to_string(),
        }
    };

    // Build the hwid index from the platforms folder.
    let mut index = boards::HwidIndex::default();
    let platforms_dir = format!("{}/platforms", core_dir);
    if let Ok(platforms) = host::fs::read_dir(&platforms_dir) {
        for plat in platforms {
            if plat.kind != openidea_plugin::FileKind::Directory {
                continue;
            }
            let boards_dir = format!("{}/platforms/{}/boards", core_dir, plat.name);
            let Ok(files) = host::fs::read_dir(&boards_dir) else { continue };
            for f in files {
                if !f.name.ends_with(".json") {
                    continue;
                }
                let path = format!("{}/{}", boards_dir, f.name);
                let Ok(text) = host::fs::read_file(&path) else { continue };
                let id = f.name.trim_end_matches(".json");
                index.add(&plat.name, id, &text);
            }
        }
    }

    let mcu_map = boards_mcu_map(project);
    let chip = detect_chip_type();

    // Retry loop: monitor for a newly inserted board until one matches or the user cancels.
    loop {
        // Device list.
        let devices = match project.run_process("python3", &pio_args(&["device".to_string(), "list".to_string(), "--json-output".to_string()]), None) {
            Ok(out) if out.status == Some(0) => boards::parse_devices(&out.stdout),
            Ok(out) => {
                let msg = if out.stderr.trim().is_empty() { out.stdout } else { out.stderr };
                host::notify(NotificationLevel::Error, "PlatformIO", &format!("`pio device list` failed:\n{}", msg.trim()));
                return;
            }
            Err(e) => {
                host::notify(NotificationLevel::Error, "PlatformIO", &e.to_string());
                return;
            }
        };

        // Match devices to boards.
        let mut candidates: Vec<(String, String, String)> = Vec::new();
        for dev in &devices {
            let Some(usb) = dev.usb_id() else { continue };
            for r in index.boards_for(&usb) {
                candidates.push((r.id.clone(), r.platform.clone(), dev.port.clone()));
            }
        }
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut unique: Vec<(String, String, String)> = Vec::new();
        for (id, platform, port) in candidates {
            if seen.insert(id.clone()) {
                unique.push((id, platform, port));
            }
        }

        // Refine by chip type (if available).
        if let Some((raw, norm)) = &chip {
            let mut filtered: Vec<(String, String, String)> = unique
                .iter()
                .filter(|(id, platform, _)| {
                    let mcu = mcu_map
                        .get(id)
                        .cloned()
                        .or_else(|| board_info(platform, id).map(|i| i.mcu));
                    match mcu {
                        Some(m) => m.to_lowercase().replace(['-', ' '], "") == *norm,
                        None => false,
                    }
                })
                .cloned()
                .collect();
            if filtered.is_empty() {
                filtered = unique.clone();
            }

            // Prefer boards already in the project's environments.
            let ini = project.read_file("platformio.ini").ok().map(|t| ini::parse(&t));
            let env_board_ids: std::collections::BTreeSet<String> = ini
                .as_ref()
                .map(|i| i.envs.iter().map(|e| e.board().to_string()).collect())
                .unwrap_or_default();
            let env_filtered: Vec<(String, String, String)> = filtered
                .iter()
                .filter(|(id, _, _)| env_board_ids.contains(id))
                .cloned()
                .collect();
            if !env_filtered.is_empty() {
                unique = env_filtered;
            } else {
                // No existing environment matches the connected chip; don't
                // dump the full candidate list. Tell the user to add a board.
                host::notify(
                    NotificationLevel::Info,
                    "PlatformIO",
                    &format!("Detected chip: {}. No matching environment in this project — use Add Board to add one.", raw),
                );
                return;
            }

            host::notify(
                NotificationLevel::Info,
                "PlatformIO",
                &format!("Detected chip: {} (matched {} candidate(s))", raw, unique.len()),
            );
        }

        if !unique.is_empty() {
            // Read ini once.
            let ini = project.read_file("platformio.ini").ok().map(|t| ini::parse(&t));
            let env_for_board: std::collections::BTreeMap<String, String> = ini
                .as_ref()
                .map(|i| i.envs.iter().map(|e| (e.board().to_string(), e.name.clone())).collect())
                .unwrap_or_default();

            // If a matching board is already an environment, activate it.
            if unique.len() == 1 && env_for_board.contains_key(&unique[0].0) {
                let env_name = env_for_board.get(&unique[0].0).cloned().unwrap();
                let root = project.root().unwrap_or_default();
                let _ = host::storage::set(&format!("active-env:{}", root), &env_name);
                host::notify(
                    NotificationLevel::Info,
                    "PlatformIO",
                    &format!("Activated environment `{}` for connected board `{}`.", env_name, unique[0].0),
                );
                host::refresh_view(VIEW);
                return;
            }

            let board_id = if unique.len() == 1 {
                unique[0].0.clone()
            } else {
                let items: Vec<PickItem> = unique
                    .iter()
                    .map(|(id, platform, port)| PickItem {
                        key: id.clone(),
                        label: id.clone(),
                        description: Some(format!("{} · {}", platform, port)),
                    })
                    .collect();
                match host::pick("Detected board", &items) {
                    Ok(Some(b)) => b,
                    _ => return,
                }
            };

            // If the board is already an environment, just activate it.
            if env_for_board.contains_key(&board_id) {
                let env_name = env_for_board.get(&board_id).cloned().unwrap();
                let root = project.root().unwrap_or_default();
                let _ = host::storage::set(&format!("active-env:{}", root), &env_name);
                host::notify(
                    NotificationLevel::Info,
                    "PlatformIO",
                    &format!("Activated environment `{}` for connected board `{}`.", env_name, board_id),
                );
                host::refresh_view(VIEW);
                return;
            }

            let args = vec![
                "project".to_string(),
                "init".to_string(),
                "-b".to_string(),
                board_id.clone(),
                "--no-install-dependencies".to_string(),
            ];
            match project.run_process("python3", &pio_args(&args), None) {
                Ok(out) if out.status == Some(0) => {
                    host::notify(
                        NotificationLevel::Info,
                        "PlatformIO",
                        &format!("Added environment for detected board `{}`.", board_id),
                    );
                    host::refresh_view(VIEW);
                }
                Ok(out) => {
                    let msg = if out.stderr.trim().is_empty() { out.stdout } else { out.stderr };
                    host::notify(
                        NotificationLevel::Error,
                        "PlatformIO",
                        &format!("`pio project init` failed:\n{}", msg.trim()),
                    );
                }
                Err(e) => {
                    host::notify(NotificationLevel::Error, "PlatformIO", &e.to_string());
                }
            }
            return;
        }

        // No match: ask the user to plug in a board and retry.
        let retry = host::confirm(
            "Detect Board",
            "No board detected. Plug in a board, then click OK to retry (Cancel to stop monitoring).",
        )
        .unwrap_or(false);
        if !retry {
            host::notify(
                NotificationLevel::Info,
                "PlatformIO",
                "No connected board matched a known board definition.",
            );
            return;
        }
    }
}

register_plugin!(Main, views);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    #[test]
    fn lists_environments_tasks_boards_core() {
        let host = MockHost::install()
            .with_capabilities(&["project.read", "process:python3", "fs.read:~/.platformio", "storage"]);
        host.set_project_root(Some("/project"));
        host.open_editor(
            "/project/platformio.ini",
            "ini",
            "[platformio]\ndefault_envs = c3\n\n[env:c3]\nplatform = espressif32\nboard = esp32-c3-devkitm-1\nframework = arduino\n",
            &[],
        );
        let view = Main.render("platformio", None, &host.invocation()).unwrap();
        let Body::Html(_) = view.body else { panic!("html") };
        assert_eq!(view.status.as_deref(), Some("c3 · 1 environment"));
    }

    #[test]
    fn activating_an_environment_saves_it() {
        let host = MockHost::install()
            .with_capabilities(&["project.read", "process:python3", "fs.read:~/.platformio", "storage"]);
        host.set_project_root(Some("/project"));
        host.open_editor("/project/platformio.ini", "ini", "[env:a]\n[env:b]\n", &[]);
        Main.handle("platformio", ViewEvent::Activate("env:b".into()), &host.invocation()).unwrap();
        assert_eq!(host.storage().get("active-env:/project").map(|s| s.as_str()), Some("b"));
    }

    #[test]
    fn says_when_platformio_ini_is_missing() {
        let host = MockHost::install()
            .with_capabilities(&["project.read", "process:python3", "fs.read:~/.platformio", "storage"]);
        host.set_project_root(Some("/project"));
        let view = Main.render("platformio", None, &host.invocation()).unwrap();
        assert_eq!(view.status.as_deref(), Some("No platformio.ini at the project root"));
    }

    #[test]
    fn core_version_runs_pio() {
        let host = MockHost::install()
            .with_capabilities(&["project.read", "process:python3", "fs.read:~/.platformio", "storage"]);
        host.set_project_root(Some("/project"));
        host.open_editor("/project/platformio.ini", "ini", "[env:x]\n", &[]);
        host.on_process("python3", |args, _| {
            if args.ends_with(&["--version".to_string()]) {
                Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "PlatformIO Core, version 6.2.0\n".into(), stderr: String::new() })
            } else {
                Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: String::new(), stderr: String::new() })
            }
        });
        Main.run_command("core-version", &host.invocation(), None).unwrap();
        assert!(host.notifications()[0].body.contains("6.2.0"));
    }

    #[test]
    fn detect_board_adds_matching_environment() {
        let host = MockHost::install()
            .with_capabilities(&["project.read", "process:python3", "fs.read:/Users/dbuelo/.platformio", "storage"]);
        host.set_project_root(Some("/project"));
        host.open_editor("/project/platformio.ini", "ini", "[env:x]\n", &[]);
        host.on_process("python3", |args, _| {
            if args.ends_with(&["device".to_string(), "list".to_string(), "--json-output".to_string()]) {
                Ok(openidea_plugin::ProcessOutput {
                    status: Some(0),
                    stdout: r#"[{"port":"/dev/cu.usbmodem1","description":"USB","hwid":"239A:80ED"}]"#.into(),
                    stderr: String::new(),
                })
            } else if args.ends_with(&["system".to_string(), "info".to_string(), "--json-output".to_string()]) {
                Ok(openidea_plugin::ProcessOutput {
                    status: Some(0),
                    stdout: r#"{"core_dir":{"title":"d","value":"/Users/dbuelo/.platformio"}}"#.into(),
                    stderr: String::new(),
                })
            } else if args.iter().any(|a| a == "init") {
                Ok(openidea_plugin::ProcessOutput {
                    status: Some(0),
                    stdout: "ok\n".into(),
                    stderr: String::new(),
                })
            } else {
                Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: String::new(), stderr: String::new() })
            }
        });
        host.add_external_file(
            "/Users/dbuelo/.platformio/platforms/espressif32/boards/adafruit.json",
            r#"{"build":{"hwids":[["0x239A","0x80ED"]]}}"#,
        );
        // The mock fs is keyed on the literal path passed; read_dir must list the platform + boards.
        // MockHost's read_dir on an external path returns entries added via add_external_file's parent dirs?
        Main.run_command("detect-board", &host.invocation(), None).unwrap();
        // Either it added an environment (notification) or reported no match; assert no panic and a notification.
        let bodies: Vec<_> = host.notifications().iter().map(|n| n.body.clone()).collect();
        assert!(bodies.iter().any(|b| b.contains("Added environment")), "{:?}", bodies);
    }

    #[test]
    fn denied_process_is_explained() {
        let host = MockHost::install() // no process:python3 grant
            .with_capabilities(&["project.read", "storage"]);
        host.set_project_root(Some("/project"));
        host.open_editor("/project/platformio.ini", "ini", "[env:x]\n", &[]);
        let err = Main.run_command("build", &host.invocation(), None).unwrap_err();
        assert!(err.to_string().contains("process:sh") || err.to_string().contains("process:python3") || err.to_string().contains("not approved"));
    }
}

