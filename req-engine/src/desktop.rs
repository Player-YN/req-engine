//! Native desktop window (WebView2 on Windows) hosting the local board UI.
//!
//! Starts the HTTP server (API + static `web/`) then opens an OS window —
//! not a browser tab.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{WebContext, WebViewBuilder};

/// Events posted from WebView IPC / folder picker / window chrome.
#[derive(Debug, Clone)]
enum DesktopEvent {
    /// Absolute path or empty if cancelled.
    FolderPicked(String),
    WinMinimize,
    WinMaximizeToggle,
    WinClose,
    WinDrag,
    /// Restore window from the tray.
    TrayShow,
    /// Real process exit (tray menu only).
    TrayQuit,
}

use crate::db;
use crate::paths::{self, resolve_home};
use crate::services::seed::seed_demo_data;
use crate::services::tokens::generate_bootstrap_tokens;

/// Append a line to `home/desktop.log` and also print it.
fn log_line(home: &Path, msg: &str) {
    println!("{msg}");
    let path = home.join("desktop.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}", msg);
    }
}

/// Show a blocking error dialog on Windows so double-click users see failures.
#[cfg(windows)]
fn alert_error(title: &str, msg: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(
            hwnd: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            flags: u32,
        ) -> i32;
    }

    let text: Vec<u16> = OsStr::new(msg)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let caption: Vec<u16> = OsStr::new(title)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // MB_OK | MB_ICONERROR
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            0x0000_0010,
        );
    }
}

#[cfg(not(windows))]
fn alert_error(_title: &str, _msg: &str) {}

/// Resolve directory that contains `index.html` for the Windows board.
pub fn find_web_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("web"));
            // target/debug/../web or target/release/../web
            candidates.push(dir.join("..").join("web"));
            candidates.push(dir.join("..").join("..").join("web"));
        }
    }
    // Dev: crate web/
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web"));

    for c in candidates {
        let c = c.canonicalize().unwrap_or(c);
        if c.join("index.html").is_file() {
            return Some(c);
        }
    }
    None
}

fn read_admin_token(home: &Path) -> Option<String> {
    let path = paths::tokens_path(home);
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("admin=") {
            let t = rest.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

fn ensure_initialized(home: &Path, seed_if_missing: bool) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = paths::db_path(home);
    if db_path.exists() {
        return Ok(());
    }
    fs::create_dir_all(home)?;
    let conn = db::open_and_migrate(&db_path)?;
    let tokens = generate_bootstrap_tokens(&conn)?;
    let tokens_file = paths::tokens_path(home);
    let mut f = fs::File::create(&tokens_file)?;
    writeln!(f, "# req-engine bootstrap tokens — treat as secrets")?;
    writeln!(f, "# Generated at {}", chrono::Utc::now().to_rfc3339())?;
    writeln!(f)?;
    for t in &tokens {
        writeln!(f, "{}={}", t.role.as_str(), t.plaintext)?;
    }
    if seed_if_missing {
        let n = seed_demo_data(&conn)?;
        log_line(
            home,
            &format!(
                "desktop: auto-init + seed ({}/{} projects)",
                n.projects_created, n.projects_skipped
            ),
        );
    } else {
        log_line(home, "desktop: auto-init (no seed)");
    }
    log_line(home, &format!("  home:   {}", home.display()));
    log_line(home, &format!("  tokens: {}", tokens_file.display()));
    Ok(())
}

/// Probe whether something is already accepting TCP on host:port.
fn port_open(host: &str, port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::new(
            host.parse().unwrap_or_else(|_| [127, 0, 0, 1].into()),
            port,
        ),
        Duration::from_millis(150),
    )
    .is_ok()
}

/// Find a free port starting at `preferred`, up through preferred+20.
fn pick_port(host: &str, preferred: u16) -> Result<u16, Box<dyn std::error::Error>> {
    let host_ip: std::net::IpAddr = host
        .parse()
        .map_err(|e| format!("invalid host {host}: {e}"))?;
    for offset in 0u16..21 {
        let port = preferred.saturating_add(offset);
        let addr = SocketAddr::new(host_ip, port);
        match TcpListener::bind(addr) {
            Ok(listener) => {
                // Drop immediately; the real server rebinds. Brief race is OK for local use.
                drop(listener);
                return Ok(port);
            }
            Err(e) => {
                if offset == 20 {
                    return Err(format!(
                        "no free port in {preferred}..{} (last error: {e})",
                        preferred + 20
                    )
                    .into());
                }
            }
        }
    }
    Err("port pick failed".into())
}

fn wait_for_http(host: &str, port: u16, attempts: u32) -> bool {
    let addr = format!("{host}:{port}");
    for _ in 0..attempts {
        if let Ok(mut stream) = TcpStream::connect(&addr) {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
            let req = format!("GET /v1/health HTTP/1.0\r\nHost: {host}\r\n\r\n");
            if stream.write_all(req.as_bytes()).is_ok() {
                let mut buf = [0u8; 256];
                if let Ok(n) = stream.read(&mut buf) {
                    let body = String::from_utf8_lossy(&buf[..n]);
                    if body.contains("req-engine") {
                        return true;
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Best-effort WebView2 Runtime presence check (Windows).
/// Missing runtime → clear error instead of a blank process crash.
fn check_webview2_runtime() -> Result<(), String> {
    #[cfg(windows)]
    {
        let candidates = [
            r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application",
            r"C:\Program Files\Microsoft\EdgeWebView\Application",
            r"C:\Program Files (x86)\Microsoft\Edge\Application",
            r"C:\Program Files\Microsoft\Edge\Application",
        ];
        if candidates.iter().any(|p| Path::new(p).is_dir()) {
            return Ok(());
        }
        // Evergreen installer registry (pv = version string when installed)
        let reg_keys = [
            r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
            r"HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
            r"HKCU\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        ];
        for key in reg_keys {
            let out = std::process::Command::new("reg")
                .args(["query", key, "/v", "pv"])
                .output();
            if let Ok(o) = out {
                if o.status.success() {
                    return Ok(());
                }
            }
        }
        return Err(
            "未检测到 Microsoft Edge WebView2 Runtime。\n\n\
             请安装 Evergreen Runtime 后重试：\n\
             https://developer.microsoft.com/microsoft-edge/webview2/\n\n\
             （Windows 10/11 多数机器已预装。）"
                .into(),
        );
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

/// Double-click launcher sets REQ_ENGINE_SILENT=1 so the console host stays hidden.
fn detach_console_if_silent() {
    let silent = std::env::var("REQ_ENGINE_SILENT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !silent {
        return;
    }
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn FreeConsole() -> i32;
        }
        unsafe {
            let _ = FreeConsole();
        }
    }
}

/// Launch API+UI server and a native WebView window. Blocks until the window closes.
pub fn run(
    home_override: Option<PathBuf>,
    host: String,
    port: u16,
    seed_if_missing: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    detach_console_if_silent();
    let home = home_override.unwrap_or_else(resolve_home);
    fs::create_dir_all(&home)?;

    // Truncate previous log for this session.
    let _ = fs::write(home.join("desktop.log"), format!(
        "# req-engine desktop log {}\n",
        chrono::Utc::now().to_rfc3339()
    ));

    if let Err(e) = check_webview2_runtime() {
        log_line(&home, &e);
        alert_error("Req-Engine", &e);
        return Err(e.into());
    }

    if let Err(e) = ensure_initialized(&home, seed_if_missing) {
        let msg = format!("init failed: {e}");
        log_line(&home, &msg);
        alert_error("Req-Engine", &msg);
        return Err(e);
    }

    let web_dir = find_web_dir();
    if let Some(ref d) = web_dir {
        log_line(&home, &format!("UI dir: {}", d.display()));
    } else {
        log_line(
            &home,
            "warning: no web/index.html found; window may show API JSON only",
        );
    }

    let port = match pick_port(&host, port) {
        Ok(p) => {
            if p != port {
                log_line(
                    &home,
                    &format!("port {port} busy; using {p} instead"),
                );
            }
            p
        }
        Err(e) => {
            let msg = bind_failure_message(&e.to_string(), port_open(&host, port));
            log_line(&home, &msg);
            alert_error("Req-Engine", &msg);
            return Err(msg.into());
        }
    };

    let db_path = paths::db_path(&home);
    let conn = match db::open_and_migrate(&db_path) {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("database open failed: {e}");
            log_line(&home, &msg);
            alert_error("Req-Engine", &msg);
            return Err(e.into());
        }
    };

    let web_for_server = web_dir.clone();
    let host_c = host.clone();
    let running = Arc::new(AtomicBool::new(true));
    let running_bg = running.clone();
    let home_log = home.clone();

    let server = thread::Builder::new()
        .name("req-engine-http".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    log_line(&home_log, &format!("desktop: failed to start tokio: {e}"));
                    return;
                }
            };
            let home_srv = home_log.clone();
            let res = rt.block_on(async move {
                crate::http::serve_with_static(conn, &host_c, port, web_for_server, home_srv).await
            });
            if let Err(e) = res {
                if running_bg.load(Ordering::SeqCst) {
                    log_line(&home_log, &format!("desktop: server stopped: {e}"));
                }
            }
        })?;

    if !wait_for_http(&host, port, 100) {
        let msg = format!(
            "server did not become ready on http://{host}:{port}\nSee {}",
            home.join("desktop.log").display()
        );
        log_line(&home, &msg);
        alert_error("Req-Engine", &msg);
        return Err(msg.into());
    }

    open_window(&home, &host, port, running, Some(server))
}

/// Never attach the WebView (and admin token) to an unknown listener.
fn bind_failure_message(pick_err: &str, someone_listening: bool) -> String {
    if someone_listening {
        format!(
            "cannot bind HTTP port ({pick_err}). Refusing to attach the window to whatever is already listening — that would send the admin token to a foreign process."
        )
    } else {
        format!("cannot bind HTTP port: {pick_err}")
    }
}

fn html_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Read the app shell HTML from disk (always the latest file — no WebView HTTP cache).
fn load_shell_html() -> Result<String, Box<dyn std::error::Error>> {
    let dir = find_web_dir().ok_or("web/index.html not found near the executable")?;
    let path = dir.join("index.html");
    let html =
        fs::read_to_string(&path).map_err(|e| format!("read UI {}: {e}", path.display()))?;
    if looks_like_nested_mock_ui(&html) {
        return Err(format!(
            "UI file still contains the nested-window mock (desk-caption): {}",
            path.display()
        )
        .into());
    }
    Ok(html)
}

/// Old static mocks used `desk-caption` / nested OS-window chrome.
/// The live shell's frameless `.titlebar` is real product UI, not a mock.
fn looks_like_nested_mock_ui(html: &str) -> bool {
    html.contains("desk-caption") || html.contains("desk-window") || html.contains("nested-mock")
}

fn open_window(
    home: &Path,
    host: &str,
    port: u16,
    running: Arc<AtomicBool>,
    server: Option<thread::JoinHandle<()>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let token = read_admin_token(home).unwrap_or_default();
    // Prefer 127.0.0.1 in the browser even if bind host was 0.0.0.0
    let browse_host = if host == "0.0.0.0" || host == "::" {
        "127.0.0.1"
    } else {
        host
    };
    let base = format!("http://{browse_host}:{port}/v1");

    let init_script = format!(
        r#"(function(){{
  try {{
    localStorage.setItem('req_engine_base', {base});
    localStorage.setItem('req_engine_token', {token});
    document.documentElement.setAttribute('data-shell','native');
  }} catch (e) {{}}
}})();"#,
        base = serde_json::to_string(&base).unwrap_or_else(|_| "\"http://127.0.0.1:7420/v1\"".into()),
        token = serde_json::to_string(&token).unwrap_or_else(|_| "\"\"".into()),
    );

    // IMPORTANT: do NOT navigate to http://…/index.html for the shell.
    // WebView2 caches that aggressively and users kept seeing the old "nested
    // window mock". Load the HTML document directly into the WebView so the
    // OS window *is* the app surface (same pattern as Tauri/Electron).
    let mut html = load_shell_html().map_err(|e| {
        let msg = e.to_string();
        log_line(home, &msg);
        alert_error("Req-Engine", &msg);
        e
    })?;
    // Inject bootstrap into the document itself (localStorage init-script timing
    // is unreliable with with_html → empty token → "未连接" + empty board).
    let exe_path = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let home_s = home.display().to_string();
    let boot_obj = serde_json::json!({
        "base": base,
        "token": token,
        "home": home_s,
        "exe": exe_path,
        "roles": {
            "discuss": { "seat": "discuss", "title": "讨论 Agent", "mcp_role": "planner" },
            "build": { "seat": "build", "title": "实现 Agent", "mcp_role": "foreman" }
        }
    });
    let boot_meta = format!(
        r#"<meta name="req-bootstrap" content="{}" />"#,
        html_escape_attr(&boot_obj.to_string())
    );
    let boot_js = format!(
        r#"<script>window.__REQ_BOOTSTRAP__={};window.__REQ_DESKTOP__=true;</script>"#,
        boot_obj
    );
    let inject = format!("{boot_meta}{boot_js}");
    if html.contains("</head>") {
        html = html.replacen("</head>", &format!("{inject}</head>"), 1);
    } else {
        html = format!("{inject}{html}");
    }
    log_line(
        home,
        &format!(
            "shell: with_html ({} bytes) api={base} token={}",
            html.len(),
            if token.is_empty() { "missing" } else { "ok" }
        ),
    );

    let event_loop = EventLoopBuilder::<DesktopEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    // Frameless + self-drawn titlebar (Teams-like integrated chrome).
    let window = WindowBuilder::new()
        .with_title("需求引擎")
        .with_inner_size(tao::dpi::LogicalSize::new(1280.0, 800.0))
        .with_decorations(false)
        .with_resizable(true)
        .with_visible(true)
        .with_focused(true)
        .build(&event_loop)
        .map_err(|e| {
            let msg = format!("window create failed: {e}");
            log_line(home, &msg);
            alert_error("Req-Engine", &msg);
            msg
        })?;

    // Persist WebView2 profile under home (localStorage only — shell is not served via HTTP).
    let wv_data = home.join("webview-data");
    let _ = fs::create_dir_all(&wv_data);
    // Drop HTTP cache from older builds that navigated to http://127.0.0.1:7420/
    for rel in [
        "EBWebView/Default/Cache",
        "EBWebView/Default/Code Cache",
        "EBWebView/Default/Service Worker",
    ] {
        let p = wv_data.join(rel);
        let _ = fs::remove_dir_all(&p);
    }
    let mut web_context = WebContext::new(Some(wv_data));

    // Mark desktop shell for JS (native folder dialog via ipc).
    let init_desktop = format!(
        "{init_script}\nwindow.__REQ_DESKTOP__=true;"
    );

    let proxy_ipc = proxy.clone();
    let webview = WebViewBuilder::with_web_context(&mut web_context)
        .with_html(html)
        .with_initialization_script(&init_desktop)
        .with_focused(true)
        .with_ipc_handler(move |req| {
            let body = req.body().trim();
            match body {
                "pick_folder" => {
                    let path = rfd::FileDialog::new()
                        .set_title("选择项目文件夹")
                        .pick_folder()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let _ = proxy_ipc.send_event(DesktopEvent::FolderPicked(path));
                }
                "win_minimize" => {
                    let _ = proxy_ipc.send_event(DesktopEvent::WinMinimize);
                }
                "win_maximize" => {
                    let _ = proxy_ipc.send_event(DesktopEvent::WinMaximizeToggle);
                }
                "win_close" => {
                    let _ = proxy_ipc.send_event(DesktopEvent::WinClose);
                }
                "win_quit" => {
                    let _ = proxy_ipc.send_event(DesktopEvent::TrayQuit);
                }
                "win_drag" => {
                    let _ = proxy_ipc.send_event(DesktopEvent::WinDrag);
                }
                _ => {}
            }
        })
        .build(&window)
        .map_err(|e| {
            let msg = format!(
                "WebView2 failed: {e}\n\nInstall Microsoft Edge WebView2 Runtime:\nhttps://developer.microsoft.com/microsoft-edge/webview2/"
            );
            log_line(home, &msg);
            alert_error("Req-Engine", &msg);
            msg
        })?;

    log_line(
        home,
        &format!("desktop window open (native frameless shell) → API {base}"),
    );
    if token.is_empty() {
        log_line(
            home,
            "warning: no admin= token in tokens.txt; paste token in UI Settings",
        );
    }

    let _server = server;
    let _ctx = web_context; // keep context alive for the event loop lifetime
    let home_for_loop = home.to_path_buf();

    #[cfg(windows)]
    let mut tray_holder: Option<tray_icon::TrayIcon> = None;
    #[cfg(windows)]
    let mut tray_tried = false;
    let mut tray_ok = false;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        // Windows: create the tray on the first pump, not before run().
        // Building it earlier often "succeeds" with no visible icon.
        #[cfg(windows)]
        if !tray_tried {
            match &event {
                Event::NewEvents(_) | Event::MainEventsCleared => {
                    tray_tried = true;
                    match install_tray(proxy.clone()) {
                        Ok(t) => {
                            tray_holder = Some(t);
                            let _ = tray_holder.as_ref();
                            tray_ok = true;
                            log_line(&home_for_loop, "tray icon ready (close hides; tray 退出 kills)");
                        }
                        Err(e) => {
                            tray_ok = false;
                            log_line(
                                &home_for_loop,
                                &format!("tray icon failed (✕ will exit): {e}"),
                            );
                        }
                    }
                }
                _ => {}
            }
        }

        match event {
            Event::UserEvent(DesktopEvent::FolderPicked(path)) => {
                let js = format!(
                    "window.__onNativeFolderPicked && window.__onNativeFolderPicked({});",
                    serde_json::to_string(&path).unwrap_or_else(|_| "\"\"".into())
                );
                if let Err(e) = webview.evaluate_script(&js) {
                    eprintln!("desktop: evaluate_script folder pick: {e}");
                }
            }
            Event::UserEvent(DesktopEvent::WinMinimize) => {
                window.set_minimized(true);
            }
            Event::UserEvent(DesktopEvent::WinMaximizeToggle) => {
                window.set_maximized(!window.is_maximized());
            }
            Event::UserEvent(DesktopEvent::WinClose) => {
                if tray_ok {
                    window.set_visible(false);
                } else {
                    running.store(false, Ordering::SeqCst);
                    log_line(&home_for_loop, "close without tray — process exiting");
                    std::process::exit(0);
                }
            }
            Event::UserEvent(DesktopEvent::TrayShow) => {
                window.set_visible(true);
                window.set_minimized(false);
                window.set_focus();
            }
            Event::UserEvent(DesktopEvent::TrayQuit) => {
                running.store(false, Ordering::SeqCst);
                // HTTP lives on another thread; dropping the event loop does not
                // abort it. Exit the process so 退出 actually ends everything.
                log_line(&home_for_loop, "quit requested — process exiting");
                std::process::exit(0);
            }
            Event::UserEvent(DesktopEvent::WinDrag) => {
                let _ = window.drag_window();
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if tray_ok {
                    window.set_visible(false);
                } else {
                    running.store(false, Ordering::SeqCst);
                    log_line(&home_for_loop, "CloseRequested without tray — process exiting");
                    std::process::exit(0);
                }
            }
            _ => {}
        }
    });
}

#[cfg(windows)]
fn install_tray(
    proxy: tao::event_loop::EventLoopProxy<DesktopEvent>,
) -> Result<tray_icon::TrayIcon, Box<dyn std::error::Error>> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id("tray-show", "打开看板", true, None);
    let quit = MenuItem::with_id("tray-quit", "退出", true, None);
    let menu = Menu::new();
    menu.append(&show)?;
    menu.append(&quit)?;

    let p_menu = proxy.clone();
    MenuEvent::set_event_handler(Some(move |ev: MenuEvent| {
        if ev.id().as_ref() == "tray-quit" {
            let _ = p_menu.send_event(DesktopEvent::TrayQuit);
        } else {
            let _ = p_menu.send_event(DesktopEvent::TrayShow);
        }
    }));
    let p_click = proxy;
    TrayIconEvent::set_event_handler(Some(move |ev| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = ev
        {
            let _ = p_click.send_event(DesktopEvent::TrayShow);
        }
    }));

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("需求引擎 — 右键退出")
        .with_icon(tray_rgba_icon())
        .build()?;
    Ok(tray)
}

#[cfg(windows)]
fn tray_rgba_icon() -> tray_icon::Icon {
    const S: u32 = 32;
    let mut rgba = vec![0u8; (S * S * 4) as usize];
    for y in 0..S {
        for x in 0..S {
            let i = ((y * S + x) * 4) as usize;
            let cx = x as i32 - 16;
            let cy = y as i32 - 16;
            let r2 = cx * cx + cy * cy;
            if r2 <= 14 * 14 {
                rgba[i] = 255;
                rgba[i + 1] = 255;
                rgba[i + 2] = 255;
            } else {
                rgba[i] = 0;
                rgba[i + 1] = 95;
                rgba[i + 2] = 184;
            }
            rgba[i + 3] = 255;
        }
    }
    tray_icon::Icon::from_rgba(rgba, S, S).expect("tray icon rgba")
}

#[cfg(test)]
mod tests {
    use super::bind_failure_message;

    #[test]
    fn occupied_port_refuses_to_attach() {
        let msg = bind_failure_message("no free port", true);
        assert!(msg.contains("Refusing to attach"), "{msg}");
        assert!(!msg.contains("attaching WebView to existing"));
    }

    #[test]
    fn live_titlebar_is_not_treated_as_mock() {
        assert!(!super::looks_like_nested_mock_ui(
            r#"<div class="titlebar" id="titlebar">需求引擎</div>"#
        ));
        assert!(super::looks_like_nested_mock_ui(
            r#"<div class="desk-caption">mock</div>"#
        ));
    }

    #[test]
    fn bind_failure_without_listener_is_plain_error() {
        let msg = bind_failure_message("no free port", false);
        assert!(msg.contains("cannot bind HTTP port"));
        assert!(!msg.contains("Refusing to attach"));
    }
}
