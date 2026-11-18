//! Layered configuration.
//!
//! Every setting is declared as an [`Opt`] (key, type, default, help). Values come from, lowest
//! to highest priority:
//!
//! 1. the declared default
//! 2. defaults set by the embedding program (`App::default_value`)
//! 3. **environment variables** (`TUNIC_ROUTER_PORTS_HTTP`, `.` and `-` become `_`)
//! 4. **`tunic.toml`**
//! 5. **command line** (`--router.ports.http 9000`, `--enable drive`, ...)
//!
//! Unknown keys are rejected so typos do not silently do nothing.

use std::collections::BTreeMap;

use utils::{Value, toml};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bool,
    /// `false` = off, `true` = on with the default port, a number = on with that port (0 = any free port).
    Port,
    Int,
    Str,
    /// List of strings (comma separated in env/cli, array in toml).
    List,
    /// `key = value` strings. Keys are free-form (`"example.com" = "localhost:2455"`).
    Map,
}

#[derive(Clone, Debug)]
pub struct Opt {
    pub key: String,
    pub kind: Kind,
    pub default: Value,
    pub help: String,
}

impl Opt {
    pub fn new(key: &str, kind: Kind, default: Value, help: &str) -> Opt {
        Opt { key: key.into(), kind, default, help: help.into() }
    }
    pub fn bool(key: &str, default: bool, help: &str) -> Opt {
        Opt::new(key, Kind::Bool, Value::Bool(default), help)
    }
    pub fn port(key: &str, default: Value, help: &str) -> Opt {
        Opt::new(key, Kind::Port, default, help)
    }
    pub fn int(key: &str, default: Option<i64>, help: &str) -> Opt {
        Opt::new(key, Kind::Int, default.map(Value::Int).unwrap_or(Value::Null), help)
    }
    pub fn str(key: &str, default: Option<&str>, help: &str) -> Opt {
        Opt::new(key, Kind::Str, default.map(|d| Value::Str(d.into())).unwrap_or(Value::Null), help)
    }
    pub fn list(key: &str, help: &str) -> Opt {
        Opt::new(key, Kind::List, Value::Array(vec![]), help)
    }
    pub fn map(key: &str, help: &str) -> Opt {
        Opt::new(key, Kind::Map, Value::object(), help)
    }
    /// `TUNIC_ROUTER_PORTS_HTTP`
    pub fn env_name(&self) -> String {
        env_name(&self.key)
    }
}

pub fn env_name(key: &str) -> String {
    format!("TUNIC_{}", key.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect::<String>())
}

/// Resolved settings plus the table of declared options.
#[derive(Clone, Debug)]
pub struct Config {
    opts: BTreeMap<String, Opt>,
    values: Value,
}

fn path(key: &str) -> Vec<String> {
    toml::split_key(key).unwrap_or_else(|_| vec![key.to_string()])
}

fn parse_scalar(kind: Kind, key: &str, s: &str) -> Result<Value, String> {
    match kind {
        Kind::Bool => match s.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(Value::Bool(true)),
            "0" | "false" | "no" | "off" => Ok(Value::Bool(false)),
            _ => Err(format!("`{key}`: `{s}` is not a boolean (use true/false)")),
        },
        Kind::Port => match s.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(Value::Bool(true)),
            "false" | "no" | "off" => Ok(Value::Bool(false)),
            n => n.parse::<u16>().map(|p| Value::Int(p as i64)).map_err(|_| format!("`{key}`: `{s}` must be true, false or a port number")),
        },
        Kind::Int => s.trim().parse::<i64>().map(Value::Int).map_err(|_| format!("`{key}`: `{s}` is not a number")),
        Kind::Str => Ok(Value::Str(s.to_string())),
        Kind::List => Ok(Value::Array(s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(|x| Value::Str(x.into())).collect())),
        Kind::Map => {
            let mut o = Value::object();
            for pair in s.split(',').map(str::trim).filter(|x| !x.is_empty()) {
                let (k, v) = pair.split_once('=').ok_or_else(|| format!("`{key}`: `{pair}` must look like name=value"))?;
                o.set(k.trim().trim_matches('"'), v.trim());
            }
            Ok(o)
        }
    }
}

/// Check (and lightly coerce) a value that came from a toml file.
fn coerce(kind: Kind, key: &str, v: &Value) -> Result<Value, String> {
    let bad = |what: &str| Err(format!("`{key}` must be {what}"));
    match (kind, v) {
        (Kind::Port, Value::Bool(_)) => Ok(v.clone()),
        (Kind::Port, Value::Int(p)) if (0..=65535).contains(p) => Ok(v.clone()),
        (Kind::Port, _) => bad("true, false or a port number (0-65535)"),
        (Kind::Bool, Value::Bool(_)) | (Kind::Int, Value::Int(_)) | (Kind::Str, Value::Str(_)) => Ok(v.clone()),
        (Kind::Str, Value::Int(i)) => Ok(Value::Str(i.to_string())),
        (Kind::Bool, _) => bad("true or false"),
        (Kind::Int, _) => bad("a whole number"),
        (Kind::Str, _) => bad("a string"),
        (Kind::List, Value::Str(s)) => parse_scalar(Kind::List, key, s),
        (Kind::List, Value::Array(a)) => {
            let mut out = vec![];
            for x in a {
                match x {
                    Value::Str(_) => out.push(x.clone()),
                    Value::Int(i) => out.push(Value::Str(i.to_string())),
                    _ => return bad("a list of strings"),
                }
            }
            Ok(Value::Array(out))
        }
        (Kind::List, _) => bad("a list of strings"),
        (Kind::Map, Value::Object(m)) => {
            let mut o = Value::object();
            for (k, x) in m {
                match x {
                    Value::Str(s) => o.set(k.as_str(), s.as_str()),
                    _ => return Err(format!("`{key}.{k}` must be a string")),
                }
            }
            Ok(o)
        }
        (Kind::Map, _) => bad("a table of name = \"value\""),
    }
}

impl Config {
    pub fn new(opts: Vec<Opt>) -> Config {
        Config { opts: opts.into_iter().map(|o| (o.key.clone(), o)).collect(), values: Value::object() }
    }

    pub fn options(&self) -> impl Iterator<Item = &Opt> {
        self.opts.values()
    }

    pub fn option(&self, key: &str) -> Option<&Opt> {
        self.opts.get(key)
    }

    /// Declare more options (later declarations of the same key replace earlier ones).
    pub fn declare(&mut self, opts: impl IntoIterator<Item = Opt>) {
        for o in opts {
            self.opts.insert(o.key.clone(), o);
        }
    }

    /// Change the default of a declared option (used by programs embedding tunic).
    pub fn set_default(&mut self, key: &str, v: Value) -> Result<(), String> {
        let o = self.opts.get_mut(key).ok_or_else(|| format!("unknown setting `{key}`"))?;
        o.default = coerce(o.kind, key, &v)?;
        Ok(())
    }

    fn set_raw(&mut self, key: &str, v: Value) {
        self.values.set_path(&path(key), v);
    }

    /// Which declared option (if any) a flattened key belongs to. Map options own all keys below them.
    fn owner(&self, flat: &str) -> Option<&Opt> {
        if let Some(o) = self.opts.get(flat) {
            return Some(o);
        }
        self.opts.values().find(|o| o.kind == Kind::Map && flat.starts_with(&format!("{}.", o.key)))
    }

    /// Merge a parsed toml/json tree (priority is decided by the order of calls).
    pub fn layer_value(&mut self, tree: &Value, origin: &str) -> Result<(), String> {
        let mut flat = vec![];
        flatten(tree, String::new(), &self.opts, &mut flat);
        for (key, v) in flat {
            let Some(o) = self.opts.get(&key).cloned() else {
                return Err(format!("{origin}: unknown setting `{key}`{}", self.suggest(&key)));
            };
            let v = coerce(o.kind, &key, &v).map_err(|e| format!("{origin}: {e}"))?;
            self.set_raw(&key, v);
        }
        Ok(())
    }

    pub fn layer_toml(&mut self, text: &str, origin: &str) -> Result<(), String> {
        let tree = toml::parse(text).map_err(|e| format!("{origin}: {e}"))?;
        self.layer_value(&tree, origin)
    }

    /// Environment variables for all declared options (`TUNIC_*`). `env` is injected for tests.
    pub fn layer_env(&mut self, env: &dyn Fn(&str) -> Option<String>) -> Result<(), String> {
        let opts: Vec<Opt> = self.opts.values().cloned().collect();
        for o in opts {
            if let Some(raw) = env(&o.env_name()) {
                let v = parse_scalar(o.kind, &o.key, &raw).map_err(|e| format!("{}: {e}", o.env_name()))?;
                self.set_raw(&o.key, v);
            }
        }
        Ok(())
    }

    /// Command line. Returns the arguments that are not settings (`--config`, `--help`, ...) for the caller.
    pub fn layer_cli(&mut self, args: &[String]) -> Result<Vec<String>, String> {
        let mut rest = vec![];
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            i += 1;
            let Some(flag) = a.strip_prefix("--") else {
                rest.push(a.clone());
                continue;
            };
            let (name, inline) = match flag.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (flag.to_string(), None),
            };
            match name.as_str() {
                "enable" | "disable" => {
                    let f = take_value(args, &mut i, &inline, &name, "a feature name")?;
                    let key = format!("{}.enabled", f.trim_end_matches(".enabled"));
                    if !matches!(self.opts.get(&key), Some(o) if o.kind == Kind::Bool) {
                        return Err(format!("--{name}: unknown feature `{f}`{}", self.suggest(&key)));
                    }
                    self.set_raw(&key, Value::Bool(name == "enable"));
                }
                "join" => {
                    let v = take_value(args, &mut i, &inline, &name, "an address")?;
                    self.append_list("p2p.join", &v)?;
                }
                "set" => {
                    let kv = take_value(args, &mut i, &inline, &name, "key=value")?;
                    let (k, v) = kv.split_once('=').ok_or("--set needs key=value")?;
                    self.set_cli(k.trim(), v)?;
                }
                "config" | "help" | "print-config" | "version" | "list-features" => {
                    rest.push(a.clone());
                    // flags that take a value keep it with the flag
                    if name == "config" {
                        match inline {
                            Some(_) => {}
                            None => {
                                if let Some(v) = args.get(i) {
                                    rest.push(v.clone());
                                    i += 1;
                                }
                            }
                        }
                    }
                }
                other if self.opts.contains_key(other) || self.owner(other).is_some() => {
                    let o = self.owner(other).cloned().expect("checked");
                    if matches!(o.kind, Kind::Bool | Kind::Port) && inline.is_none() && !args.get(i).is_some_and(|n| !n.starts_with("--") && parse_scalar(o.kind, other, n).is_ok()) {
                        self.set_raw(other, Value::Bool(true));
                    } else {
                        let v = take_value(args, &mut i, &inline, &name, "a value")?;
                        self.set_cli(other, &v)?;
                    }
                }
                other => {
                    if let Some(neg) = other.strip_prefix("no-") {
                        if matches!(self.opts.get(neg), Some(o) if matches!(o.kind, Kind::Bool | Kind::Port)) {
                            self.set_raw(neg, Value::Bool(false));
                            continue;
                        }
                    }
                    return Err(format!("unknown option --{other}{}", self.suggest(other)));
                }
            }
        }
        Ok(rest)
    }

    fn set_cli(&mut self, key: &str, raw: &str) -> Result<(), String> {
        let o = self.owner(key).cloned().ok_or_else(|| format!("unknown setting `{key}`{}", self.suggest(key)))?;
        match o.kind {
            Kind::List => self.append_list(key, raw),
            Kind::Map if key != o.key => {
                // `--router.reverse-proxy.example.com localhost:2455`
                let sub = &key[o.key.len() + 1..];
                self.set_raw(&format!("{}.\"{}\"", o.key, sub.trim_matches('"')), Value::Str(raw.into()));
                Ok(())
            }
            Kind::Map => {
                let v = parse_scalar(Kind::Map, key, raw)?;
                let mut cur = self.values.path(key).cloned().unwrap_or_else(Value::object);
                cur.merge(&v);
                self.set_raw(key, cur);
                Ok(())
            }
            k => {
                let v = parse_scalar(k, key, raw)?;
                self.set_raw(key, v);
                Ok(())
            }
        }
    }

    fn append_list(&mut self, key: &str, raw: &str) -> Result<(), String> {
        let Value::Array(new) = parse_scalar(Kind::List, key, raw)? else { unreachable!() };
        // a CLI list replaces file/env values the first time, then grows with each repetition
        let marker = format!("\0cli:{key}");
        let seen = self.values.get(&marker).is_some();
        let mut cur = if seen { self.list(key) } else { vec![] };
        cur.extend(new.iter().filter_map(|v| v.as_str().map(String::from)));
        self.set_raw(key, Value::Array(cur.into_iter().map(Value::Str).collect()));
        self.values.set(marker, true);
        Ok(())
    }

    fn suggest(&self, key: &str) -> String {
        let best = self.opts.keys().filter(|k| k.contains(key.rsplit('.').next().unwrap_or(key)) || key.contains(k.rsplit('.').next().unwrap_or(k))).take(3).cloned().collect::<Vec<_>>();
        if best.is_empty() { String::new() } else { format!(" (did you mean {}?)", best.join(", ")) }
    }

    // ---- reading

    fn lookup(&self, key: &str) -> Option<&Value> {
        match self.values.path(key) {
            Some(v) if !v.is_null() => Some(v),
            _ => self.opts.get(key).map(|o| &o.default).filter(|d| !d.is_null()),
        }
    }

    /// True when the key was set by env/toml/cli/the program (not just its default).
    pub fn is_set(&self, key: &str) -> bool {
        matches!(self.values.path(key), Some(v) if !v.is_null())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.lookup(key)
    }
    pub fn bool(&self, key: &str) -> bool {
        self.lookup(key).and_then(|v| v.as_bool()).unwrap_or(false)
    }
    /// For [`Kind::Port`] settings: `None` = off, `Some(port)`; `true` means `default_port`.
    pub fn port(&self, key: &str, default_port: u16) -> Option<u16> {
        match self.lookup(key)? {
            Value::Bool(true) => Some(default_port),
            Value::Int(p) => Some(*p as u16),
            _ => None,
        }
    }
    pub fn int(&self, key: &str) -> Option<i64> {
        self.lookup(key).and_then(|v| v.as_i64())
    }
    pub fn str(&self, key: &str) -> Option<String> {
        self.lookup(key).and_then(|v| v.as_str()).map(String::from)
    }
    pub fn list(&self, key: &str) -> Vec<String> {
        self.lookup(key).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
    }
    pub fn map(&self, key: &str) -> BTreeMap<String, String> {
        self.lookup(key).and_then(|v| v.as_object()).map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect()).unwrap_or_default()
    }

    /// Every effective setting as `key = value` lines (secrets hidden), valid toml.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for o in self.opts.values() {
            let secret = ["token", "password"].iter().any(|s| o.key.contains(s));
            match o.kind {
                Kind::Map => {
                    let m = self.map(&o.key);
                    for (k, v) in m {
                        // database.route values may look like `token@host:port`
                        let v = if secret {
                            "***".to_string()
                        } else if o.key == "database.route" {
                            v.split_once('@').map(|(_, a)| format!("***@{a}")).unwrap_or(v)
                        } else {
                            v
                        };
                        out.push_str(&format!("{}.\"{}\" = {}\n", o.key, k, Value::Str(v).to_json()));
                    }
                }
                _ => {
                    if let Some(v) = self.lookup(&o.key) {
                        let shown = if secret && !v.is_null() { Value::Str("***".into()) } else { v.clone() };
                        out.push_str(&format!("{} = {}\n", o.key, shown.to_json()));
                    }
                }
            }
        }
        out
    }

    /// Help text listing every setting, its type, default, and env name.
    pub fn help(&self) -> String {
        let mut out = String::new();
        let mut last = String::new();
        for o in self.opts.values() {
            let section = o.key.split('.').next().unwrap_or("").to_string();
            if section != last {
                out.push_str(&format!("\n[{section}]\n"));
                last = section;
            }
            let kind = match o.kind {
                Kind::Bool => "bool",
                Kind::Port => "bool or port",
                Kind::Int => "number",
                Kind::Str => "text",
                Kind::List => "list",
                Kind::Map => "name=value,...",
            };
            let def = if o.default.is_null() || matches!(&o.default, Value::Array(a) if a.is_empty()) || matches!(&o.default, Value::Object(m) if m.is_empty()) { String::new() } else { format!(" [default: {}]", o.default.to_json()) };
            out.push_str(&format!("  --{:<34} {} ({kind}){def}\n  {:<38} env {}\n", o.key, o.help, "", o.env_name()));
        }
        out
    }
}

fn take_value(args: &[String], i: &mut usize, inline: &Option<String>, name: &str, what: &str) -> Result<String, String> {
    if let Some(v) = inline {
        return Ok(v.clone());
    }
    let v = args.get(*i).ok_or_else(|| format!("--{name} needs {what}"))?.clone();
    *i += 1;
    Ok(v)
}

fn flatten(v: &Value, prefix: String, opts: &BTreeMap<String, Opt>, out: &mut Vec<(String, Value)>) {
    match v {
        Value::Object(m) if !prefix.is_empty() && opts.get(&prefix).is_some_and(|o| o.kind == Kind::Map) => {
            out.push((prefix, Value::Object(m.clone())));
        }
        Value::Object(m) => {
            for (k, x) in m {
                let key = if prefix.is_empty() { quote_seg(k) } else { format!("{prefix}.{}", quote_seg(k)) };
                flatten(x, key, opts, out);
            }
        }
        other => out.push((prefix, other.clone())),
    }
}

fn quote_seg(k: &str) -> String {
    k.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::new(vec![
            Opt::bool("router.enabled", true, "serve"),
            Opt::int("router.ports.http", Some(8080), "port"),
            Opt::str("database.token", None, "secret"),
            Opt::list("p2p.join", "peers"),
            Opt::map("router.reverse-proxy", "hosts"),
            Opt::bool("drive.enabled", true, "drive"),
        ])
    }

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn defaults_and_env_names() {
        let c = cfg();
        assert!(c.bool("router.enabled"));
        assert_eq!(c.int("router.ports.http"), Some(8080));
        assert_eq!(c.str("database.token"), None);
        assert_eq!(c.option("router.reverse-proxy").unwrap().env_name(), "TUNIC_ROUTER_REVERSE_PROXY");
    }

    #[test]
    fn priority_is_cli_over_toml_over_env() {
        let mut c = cfg();
        c.layer_env(&env(&[("TUNIC_ROUTER_PORTS_HTTP", "1111"), ("TUNIC_DATABASE_TOKEN", "from-env"), ("TUNIC_ROUTER_ENABLED", "false")])).unwrap();
        assert_eq!((c.int("router.ports.http"), c.bool("router.enabled")), (Some(1111), false), "env alone applies");
        c.layer_toml("[router]\nenabled = true\n[router.ports]\nhttp = 2222\n", "tunic.toml").unwrap();
        assert_eq!((c.int("router.ports.http"), c.bool("router.enabled")), (Some(2222), true), "toml beats env");
        assert_eq!(c.str("database.token").as_deref(), Some("from-env"), "env fills what toml leaves out");
        let rest = c.layer_cli(&["--router.ports.http".into(), "3333".into(), "--disable".into(), "drive".into(), "--config".into(), "x.toml".into()]).unwrap();
        assert_eq!(c.int("router.ports.http"), Some(3333), "cli beats toml");
        assert!(!c.bool("drive.enabled"));
        assert_eq!(rest, ["--config", "x.toml"]);
    }

    #[test]
    fn cli_forms() {
        let mut c = cfg();
        c.layer_cli(&["--router.ports.http=99".into(), "--no-router.enabled".into(), "--join".into(), "a:1".into(), "--join".into(), "b:2,c:3".into()]).unwrap();
        assert_eq!(c.int("router.ports.http"), Some(99));
        assert!(!c.bool("router.enabled"));
        assert_eq!(c.list("p2p.join"), ["a:1", "b:2", "c:3"]);
        c.layer_cli(&["--router.reverse-proxy.example.com".into(), "localhost:2455".into(), "--set".into(), "router.reverse-proxy=b.org=x:1".into()]).unwrap();
        assert_eq!(c.map("router.reverse-proxy").get("example.com").map(String::as_str), Some("localhost:2455"));
        assert_eq!(c.map("router.reverse-proxy").get("b.org").map(String::as_str), Some("x:1"));
        let mut d = cfg();
        d.layer_cli(&["--drive.enabled".into()]).unwrap();
        assert!(d.bool("drive.enabled"));
    }

    #[test]
    fn toml_tables_lists_and_maps() {
        let mut c = cfg();
        c.layer_toml("[p2p]\njoin = [\"10.0.0.1:7000\", \"10.0.0.2:7000\"]\n[router.reverse-proxy]\n\"example.com\" = \"localhost:2455\"\n", "t").unwrap();
        assert_eq!(c.list("p2p.join").len(), 2);
        assert_eq!(c.map("router.reverse-proxy")["example.com"], "localhost:2455");
    }

    #[test]
    fn errors_are_specific() {
        let mut c = cfg();
        assert!(c.layer_toml("[router]\nenabeld = true\n", "tunic.toml").unwrap_err().contains("unknown setting `router.enabeld`"));
        assert!(c.layer_toml("[router]\nenabled = \"yes\"\n", "tunic.toml").unwrap_err().contains("true or false"));
        assert!(c.layer_cli(&["--nope".into()]).unwrap_err().contains("unknown option --nope"));
        assert!(c.layer_cli(&["--enable".into(), "ghost".into()]).is_err());
        assert!(c.layer_env(&env(&[("TUNIC_ROUTER_PORTS_HTTP", "abc")])).unwrap_err().contains("not a number"));
        assert!(c.layer_cli(&["--router.ports.http".into()]).is_err());
    }

    #[test]
    fn render_hides_secrets_and_is_toml() {
        let mut c = cfg();
        c.layer_cli(&["--database.token".into(), "hunter2".into()]).unwrap();
        let r = c.render();
        assert!(!r.contains("hunter2") && r.contains("database.token = \"***\""));
        let mut again = cfg();
        again.layer_toml(&r, "round trip").unwrap();
        assert_eq!(again.int("router.ports.http"), Some(8080));
        assert!(c.help().contains("TUNIC_ROUTER_PORTS_HTTP"));
    }
}
