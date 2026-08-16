//! CLI entrypoint for `req-engine`.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use req_engine::db;
use req_engine::mcp::McpRole;
use req_engine::paths::{self, resolve_home};
use req_engine::services::seed::seed_demo_data;
use req_engine::services::tokens::generate_bootstrap_tokens;

#[derive(Parser, Debug)]
#[command(
    name = "req-engine",
    version,
    about = "Requirements Engine — verb-based requirement lifecycle (zero LLM)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliMcpRole {
    Planner,
    Foreman,
}

impl From<CliMcpRole> for McpRole {
    fn from(r: CliMcpRole) -> Self {
        match r {
            CliMcpRole::Planner => McpRole::Planner,
            CliMcpRole::Foreman => McpRole::Foreman,
        }
    }
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Create data directory, SQLite DB, run migrations, generate role tokens.
    Init {
        /// Override REQ_ENGINE_HOME for this run.
        #[arg(long, env = "REQ_ENGINE_HOME")]
        home: Option<PathBuf>,

        /// Force re-init: remove existing DB and tokens (destructive).
        #[arg(long, default_value_t = false)]
        force: bool,

        /// Also seed demo projects (demo-shop, trace-sight, req-engine, mobile-h5).
        #[arg(long, default_value_t = false)]
        seed: bool,
    },
    /// Seed demo projects into an existing DB (idempotent-ish).
    Seed {
        #[arg(long, env = "REQ_ENGINE_HOME")]
        home: Option<PathBuf>,
    },
    /// HTTP API server.
    Serve {
        /// Bind host.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Bind port.
        #[arg(long, default_value_t = 7420)]
        port: u16,

        /// Override REQ_ENGINE_HOME for this run.
        #[arg(long, env = "REQ_ENGINE_HOME")]
        home: Option<PathBuf>,
    },
    /// MCP stdio server (pair code binds project + seat).
    ///
    /// Logs go to stderr; stdout is reserved for MCP JSON-RPC.
    Mcp {
        /// Product path: per-project discuss/build pair code.
        #[arg(long)]
        pair: Option<String>,

        /// Debug only: planner | foreman (requires --token). Ignored when --pair is set.
        #[arg(long, value_enum)]
        role: Option<CliMcpRole>,

        /// Debug only: bearer token (or set REQ_ENGINE_TOKEN).
        #[arg(long, env = "REQ_ENGINE_TOKEN")]
        token: Option<String>,

        /// Override REQ_ENGINE_HOME for this run.
        #[arg(long, env = "REQ_ENGINE_HOME")]
        home: Option<PathBuf>,
    },
    /// Native desktop window (WebView) + local API — not a browser tab.
    Desktop {
        /// Bind host for the embedded HTTP server.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Bind port for the embedded HTTP server.
        #[arg(long, default_value_t = 7420)]
        port: u16,

        /// Override REQ_ENGINE_HOME for this run.
        #[arg(long, env = "REQ_ENGINE_HOME")]
        home: Option<PathBuf>,

        /// If the database is missing, also seed demo projects (off by default for product use).
        #[arg(long, default_value_t = false)]
        seed_if_missing: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init { home, force, seed } => match cmd_init(home, force, seed) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        Commands::Seed { home } => match cmd_seed(home) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        Commands::Serve { host, port, home } => match cmd_serve(home, host, port) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        Commands::Mcp {
            pair,
            role,
            token,
            home,
        } => match cmd_mcp(home, role.map(Into::into), token, pair) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                // MCP: never write protocol noise to stdout
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        Commands::Desktop {
            host,
            port,
            home,
            seed_if_missing,
        } => match req_engine::desktop::run(home, host, port, seed_if_missing) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
    }
}

fn cmd_init(
    home_override: Option<PathBuf>,
    force: bool,
    seed: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let home = home_override.unwrap_or_else(resolve_home);
    let db_path = paths::db_path(&home);
    let tokens_file = paths::tokens_path(&home);

    if force {
        if db_path.exists() {
            fs::remove_file(&db_path)?;
        }
        if tokens_file.exists() {
            fs::remove_file(&tokens_file)?;
        }
    } else if db_path.exists() {
        return Err(format!(
            "database already exists at {} (use --force to re-init)",
            db_path.display()
        )
        .into());
    }

    fs::create_dir_all(&home)?;

    let conn = db::open_and_migrate(&db_path)?;
    let tokens = generate_bootstrap_tokens(&conn)?;

    // Write plaintext tokens for local dev (gitignored).
    let mut f = fs::File::create(&tokens_file)?;
    writeln!(f, "# req-engine bootstrap tokens — treat as secrets")?;
    writeln!(f, "# Generated at {}", chrono::Utc::now().to_rfc3339())?;
    writeln!(f, "# Hash is stored in DB; plaintext only here / printed once")?;
    writeln!(f)?;
    for t in &tokens {
        writeln!(f, "# role={} name={}", t.role.as_str(), t.name)?;
        writeln!(f, "{}={}", t.role.as_str(), t.plaintext)?;
    }

    println!("req-engine initialized");
    println!("  home:   {}", home.display());
    println!("  db:     {}", db_path.display());
    println!("  tokens: {}", tokens_file.display());
    println!();
    println!("Bootstrap tokens (plaintext — save securely; hashes stored in DB):");
    for t in &tokens {
        println!("  {:8}  {}", t.role.as_str(), t.plaintext);
    }

    let _ = req_engine::services::ensure_all_project_pair_codes(&conn, &home);

    if seed {
        let report = seed_demo_data(&conn)?;
        println!();
        println!(
            "Seeded demo data: {} projects created, {} skipped, {} requirements",
            report.projects_created, report.projects_skipped, report.requirements_created
        );
        let _ = req_engine::services::ensure_all_project_pair_codes(&conn, &home);
    }

    println!();
    println!("Set REQ_ENGINE_HOME to override the default data directory.");
    println!("Start API: req-engine serve --host 127.0.0.1 --port 7420");
    println!("Start MCP: req-engine mcp --pair <disc_… or build_…> --home <dir>");

    Ok(())
}

fn cmd_seed(home_override: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let home = home_override.unwrap_or_else(resolve_home);
    let db_path = paths::db_path(&home);
    if !db_path.exists() {
        return Err(format!(
            "database not found at {} — run `req-engine init` first",
            db_path.display()
        )
        .into());
    }
    let conn = db::open_and_migrate(&db_path)?;
    let report = seed_demo_data(&conn)?;
    let _ = req_engine::services::ensure_all_project_pair_codes(&conn, &home);
    println!(
        "Seed complete: {} projects created, {} skipped, {} requirements created",
        report.projects_created, report.projects_skipped, report.requirements_created
    );
    Ok(())
}

fn cmd_serve(
    home_override: Option<PathBuf>,
    host: String,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let home = home_override.unwrap_or_else(resolve_home);
    let db_path = paths::db_path(&home);
    if !db_path.exists() {
        return Err(format!(
            "database not found at {} — run `req-engine init` first",
            db_path.display()
        )
        .into());
    }
    let conn = db::open_and_migrate(&db_path)?;
    let _ = req_engine::services::ensure_all_project_pair_codes(&conn, &home);
    println!("using db {}", db_path.display());

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(req_engine::http::serve(conn, &host, port, home))?;
    Ok(())
}

fn cmd_mcp(
    home_override: Option<PathBuf>,
    role: Option<McpRole>,
    token: Option<String>,
    pair: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(req_engine::mcp::run(home_override, role, token, pair))?;
    Ok(())
}
