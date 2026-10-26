//! `dictgen mcp-dump`: the names of the 1C platform (types, global methods and properties, members of
//! the types) read from an MCP server that is described in `.mcp.json`.
//!
//! Only names are saved, never descriptions. The server has no "list everything" call, so the
//! search is asked for every one- and two-letter prefix and then `getMembers` for every type found.
//! The result is cached in the output file; a second run asks only for what is missing.

use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use serde_json::{json, Value};

use crate::Args;

const LETTERS: &str = "абвгдежзийклмнопрстуфхцчшщэюя";
/// The kinds of the search: `type`, `method` and `property` of the server.
const KINDS: [&str; 3] = ["type", "method", "property"];
const SAVE_EVERY: usize = 100;
/// A call that is not answered in this time is skipped (the server of the platform sometimes hangs on a query).
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// The run stops (the cache is saved) after this many calls in a row without an answer.
const MAX_SILENT_CALLS: usize = 3;
const TIMED_OUT: &str = "timed out";

struct Server {
    child: Child,
    input: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl Server {
    fn start(config: &Path, name: Option<&str>) -> Result<Server, String> {
        let text = fs::read_to_string(config).map_err(|e| format!("{}: {e}", config.display()))?;
        let json: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", config.display()))?;
        let servers = json["mcpServers"].as_object().ok_or("no mcpServers in the config")?;
        let (name, spec) = match name {
            Some(n) => (n.to_string(), servers.get(n).ok_or_else(|| format!("no server '{n}' in the config"))?),
            None => servers.iter().next().map(|(k, v)| (k.clone(), v)).ok_or("the config has no servers")?,
        };
        let command = spec["command"].as_str().ok_or_else(|| format!("server '{name}' has no command"))?;
        let dir = config.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let mut cmd = Command::new(command);
        cmd.current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        for arg in spec["args"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            cmd.arg(arg);
        }
        for (key, value) in spec["env"].as_object().into_iter().flatten() {
            if let Some(value) = value.as_str() {
                cmd.env(key, value);
            }
        }
        let mut child = cmd.spawn().map_err(|e| format!("cannot start '{command}': {e}"))?;
        let input = child.stdin.take().ok_or("no stdin")?;
        let output = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        // the lines are read in a thread so that a hung server cannot hang this program
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in output.lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let mut server = Server { child, input, lines, next_id: 1 };
        server.request(
            "initialize",
            json!({"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "dictgen", "version": "1"}}),
        )?;
        server.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))?;
        Ok(server)
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        writeln!(self.input, "{message}").and_then(|_| self.input.flush()).map_err(|e| format!("the server is gone: {e}"))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        loop {
            let line = match self.lines.recv_timeout(CALL_TIMEOUT) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => return Err(format!("{method}: {TIMED_OUT}")),
                Err(RecvTimeoutError::Disconnected) => return Err("the server closed the connection".into()),
            };
            let Ok(reply) = serde_json::from_str::<Value>(line.trim()) else { continue };
            if reply["id"] != json!(id) {
                continue;
            }
            if let Some(error) = reply.get("error") {
                return Err(format!("{method}: {error}"));
            }
            return Ok(reply["result"].clone());
        }
    }

    /// The text that a tool returns.
    fn tool(&mut self, name: &str, arguments: Value) -> Result<String, String> {
        let result = self.request("tools/call", json!({"name": name, "arguments": arguments}))?;
        let text: String = result["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        // some servers return the text as a JSON string
        Ok(serde_json::from_str::<String>(&text).unwrap_or(text))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The names in the headings (`### Name`) of a reply.
pub fn names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("### "))
        .map(str::trim)
        .filter(|n| n.chars().count() >= 2 && n.chars().all(|c| c.is_alphanumeric() || c == '_'))
        .map(str::to_string)
        .collect()
}

#[derive(Default)]
struct Dump {
    server: String,
    types: BTreeSet<String>,
    methods: BTreeSet<String>,
    properties: BTreeSet<String>,
    members: BTreeSet<String>,
    done_queries: BTreeSet<String>,
    done_types: BTreeSet<String>,
}

fn strings(value: &Value) -> BTreeSet<String> {
    value.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}

impl Dump {
    fn load(path: &Path) -> Dump {
        let Ok(text) = fs::read_to_string(path) else { return Dump::default() };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { return Dump::default() };
        Dump {
            server: v["server"].as_str().unwrap_or_default().to_string(),
            types: strings(&v["types"]),
            methods: strings(&v["methods"]),
            properties: strings(&v["properties"]),
            members: strings(&v["members"]),
            done_queries: strings(&v["done_queries"]),
            done_types: strings(&v["done_types"]),
        }
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        let value = json!({
            "server": self.server, "types": self.types, "methods": self.methods, "properties": self.properties,
            "members": self.members, "done_queries": self.done_queries, "done_types": self.done_types,
        });
        let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
        fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn set(&mut self, kind: &str) -> &mut BTreeSet<String> {
        match kind {
            "type" => &mut self.types,
            "method" => &mut self.methods,
            _ => &mut self.properties,
        }
    }

    fn total(&self) -> usize {
        self.types.len() + self.methods.len() + self.properties.len() + self.members.len()
    }
}

/// The names of a dump file (all kinds, without duplicates), for `dictgen programming --platform`.
pub fn read_names(path: &Path) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut all = BTreeSet::new();
    for key in ["types", "methods", "properties", "members"] {
        all.extend(strings(&v[key]));
    }
    Ok(all.into_iter().collect())
}

pub fn run(args: &Args) -> Result<(), String> {
    let config = PathBuf::from(args.required("config")?);
    let out = PathBuf::from(args.required("out")?);
    let depth: usize = args.number("prefix-depth", 2)?;
    let budget: usize = args.number("max-calls", usize::MAX)?;
    let mut dump = Dump::load(&out);
    let mut server = Server::start(&config, args.get("server"))?;
    dump.server = args.get("server").unwrap_or("first server of the config").to_string();

    let letters: Vec<char> = LETTERS.chars().collect();
    let mut prefixes: Vec<String> = letters.iter().map(char::to_string).collect();
    if depth >= 2 {
        prefixes.extend(letters.iter().flat_map(|a| letters.iter().map(move |b| format!("{a}{b}"))));
    }

    let mut calls = 0usize;
    let mut silent = 0usize;
    // asks the server; a query that gets no answer (a hang, or the server dies on it) is skipped
    // and the server is started again; `None` is a skipped query
    let mut ask = |server: &mut Server, dump: &Dump, tool: &str, arguments: Value| -> Result<Option<String>, String> {
        match server.tool(tool, arguments) {
            Ok(reply) => {
                silent = 0;
                Ok(Some(reply))
            }
            Err(e) if e.ends_with(TIMED_OUT) || e.contains("closed the connection") || e.contains("is gone") => {
                silent += 1;
                println!("no answer to {tool} ({silent} in a row), skipped");
                if silent >= MAX_SILENT_CALLS {
                    dump.save(&out)?;
                    return Err("the server does not answer; the names found so far are saved, run again later".into());
                }
                *server = Server::start(&config, args.get("server"))?;
                Ok(None)
            }
            Err(e) => {
                dump.save(&out)?;
                Err(e)
            }
        }
    };
    'search: for prefix in &prefixes {
        for kind in KINDS {
            let key = format!("{kind}:{prefix}");
            if dump.done_queries.contains(&key) {
                continue;
            }
            if calls >= budget {
                break 'search;
            }
            calls += 1;
            if let Some(reply) = ask(&mut server, &dump, "search", json!({"query": prefix, "type": kind, "limit": 50}))? {
                dump.set(kind).extend(names(&reply));
            }
            dump.done_queries.insert(key);
            if calls.is_multiple_of(SAVE_EVERY) {
                dump.save(&out)?;
                println!("{calls} calls, {} names", dump.total());
            }
        }
    }
    let types: Vec<String> = dump.types.iter().filter(|t| !dump.done_types.contains(*t)).cloned().collect();
    for name in types {
        if calls >= budget {
            break;
        }
        calls += 1;
        if let Some(reply) = ask(&mut server, &dump, "getMembers", json!({"typeName": name}))? {
            dump.members.extend(names(&reply));
        }
        dump.done_types.insert(name);
        if calls.is_multiple_of(SAVE_EVERY) {
            dump.save(&out)?;
            println!("{calls} calls, {} names", dump.total());
        }
    }
    dump.save(&out)?;
    let left = dump.types.iter().filter(|t| !dump.done_types.contains(*t)).count();
    println!(
        "{}: {} types, {} methods, {} properties, {} members of types ({} calls now, {} types left to ask)",
        out.display(),
        dump.types.len(),
        dump.methods.len(),
        dump.properties.len(),
        dump.members.len(),
        calls,
        left
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_taken_from_headings_only() {
        let text = "# Результаты поиска: 'Запрос'\n\n### Запрос\n**Тип элемента:** Type\n**Описание:** Выполняет запрос.\n\n### HTTPСервисЗапрос\n\n## Методы\n### Выполнить\n```bsl\nВыполнить(Имя: Строка)\n```\n### Два слова\n### Х\n";
        assert_eq!(names(text), ["Запрос", "HTTPСервисЗапрос", "Выполнить"]);
    }

    #[test]
    fn a_dump_file_is_read_back_without_descriptions() {
        let path = std::env::temp_dir().join(format!("ls-dump-{}.json", std::process::id()));
        let mut dump = Dump::default();
        dump.types.insert("Запрос".into());
        dump.members.insert("Выполнить".into());
        dump.members.insert("Запрос".into());
        dump.save(&path).unwrap();
        assert_eq!(read_names(&path).unwrap(), ["Выполнить", "Запрос"]);
        assert_eq!(Dump::load(&path).types.len(), 1);
        fs::remove_file(path).unwrap();
    }
}
