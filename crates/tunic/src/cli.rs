//! Command line entry point: `tunic [--config tunic.toml] [--setting value]...`.

use crate::app::App;
use crate::config::Config;

pub const USAGE: &str = "\
tunic - distributed application platform

USAGE:
    tunic [OPTIONS]

OPTIONS:
    --config <file>          settings file (default: ./tunic.toml when it exists; env TUNIC_CONFIG)
    --enable <feature>       turn a feature on (see --list-features)
    --disable <feature>      turn a feature off
    --join <host:port>       contact this node to enter the mesh (repeatable)
    --<setting> <value>      any setting below, e.g. --router.protocols.http 9000
    --set <setting>=<value>  same, for names that are awkward on a shell
    --print-config           show the effective settings and exit
    --list-features          show the features and exit
    --help, --version

Priority: command line > tunic.toml > environment (TUNIC_ROUTER_PORTS_HTTP, ...) > defaults.
";

fn config_path(args: &[String], env: &dyn Fn(&str) -> Option<String>) -> Result<Option<String>, String> {
    let mut i = 0;
    while i < args.len() {
        if let Some(v) = args[i].strip_prefix("--config=") {
            return Ok(Some(v.to_string()));
        }
        if args[i] == "--config" {
            return args.get(i + 1).cloned().map(Some).ok_or_else(|| "--config needs a file name".to_string());
        }
        i += 1;
    }
    if let Some(p) = env("TUNIC_CONFIG") {
        return Ok(Some(p));
    }
    Ok(std::path::Path::new("tunic.toml").exists().then(|| "tunic.toml".to_string()))
}

/// Build the effective config from env, file and command line (in that order of priority, low to high).
pub fn load(app: &App, args: &[String], env: &dyn Fn(&str) -> Option<String>) -> Result<(Config, Vec<String>), String> {
    let mut c = app.config()?;
    c.layer_env(env)?;
    if let Some(path) = config_path(args, env)? {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
        c.layer_toml(&text, &path)?;
    }
    let rest = c.layer_cli(args)?;
    Ok((c, rest))
}

/// Run an app until Ctrl-C. Returns the process exit status.
pub fn run(app: App, args: Vec<String>) -> Result<(), String> {
    let env = |k: &str| std::env::var(k).ok();
    let (config, rest) = load(&app, &args, &env)?;
    let flag = |f: &str| rest.iter().any(|a| a == f);
    if flag("--help") || flag("-h") {
        println!("{USAGE}\nSETTINGS:{}", config.help());
        return Ok(());
    }
    if flag("--version") {
        println!("tunic {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if flag("--list-features") {
        for (name, desc, on) in app.feature_names() {
            let state = if config.bool(&format!("{name}.enabled")) { "on " } else { "off" };
            println!("{state} {name:<14} {desc}{}", if on { "" } else { "  (off by default)" });
        }
        return Ok(());
    }
    if flag("--print-config") {
        print!("{}", config.render());
        return Ok(());
    }
    if let Some(stray) = rest.iter().find(|a| !matches!(a.as_str(), "--config") && !a.starts_with("--config=")) {
        // the value that follows `--config` is also left over; ignore it
        let is_config_value = rest.windows(2).any(|w| w[0] == "--config" && &w[1] == stray);
        if !is_config_value {
            return Err(format!("unexpected argument `{stray}` (see --help)"));
        }
    }
    let running = app.start_with(config)?;
    running.shutdown_handle().trigger_on_ctrl_c();
    eprintln!("tunic: running [{}]", running.features().join(", "));
    for (what, addr) in running.addrs() {
        eprintln!("tunic: {what:<14} {addr}");
    }
    running.wait();
    eprintln!("tunic: stopped");
    Ok(())
}

/// `fn main() { tunic::cli::main_with(tunic::App::new()) }`
pub fn main_with(app: App) -> std::process::ExitCode {
    match run(app, std::env::args().skip(1).collect()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tunic: {e}");
            std::process::ExitCode::from(2)
        }
    }
}
