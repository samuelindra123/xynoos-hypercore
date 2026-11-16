//! Disposable source search / word-frequency indexing worker. No process targeting.
#![forbid(unsafe_code)]
use nix::sys::resource::{getrlimit, setrlimit, Resource};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, BufRead, Read, Write},
    time::{Duration, Instant},
};

const MAX_CORPUS: u64 = 4 * 1024 * 1024;
const CHUNK: usize = 32768;
const TERMS: [&str; 4] = ["memory", "pressure", "Result", "cgroup"];

fn limits() -> Result<(), Box<dyn std::error::Error>> {
    for (resource, maximum) in [
        (Resource::RLIMIT_AS, 192 * 1024 * 1024),
        (Resource::RLIMIT_CPU, 20),
        (Resource::RLIMIT_CORE, 0),
    ] {
        let (_, hard) = getrlimit(resource)?;
        let value = hard.min(maximum);
        setrlimit(resource, value, value)?;
        if getrlimit(resource)? != (value, value) {
            return Err("limit read-back mismatch".into());
        }
    }
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(60));
        std::process::exit(124);
    });
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3
        || !matches!(args[0].as_str(), "search" | "index")
        || args[1] != "--corpus-fd"
    {
        return Err("usage: xynoos-desktop search|index --corpus-fd FD".into());
    }
    let fd: u32 = args[2].parse()?;
    if fd < 3 {
        return Err("corpus must be a separate inherited regular file".into());
    }
    limits()?;
    let file = File::open(format!("/proc/self/fd/{fd}"))?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > MAX_CORPUS {
        return Err("corpus must be regular and at most 4 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_CORPUS + 1).read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_CORPUS || !bytes.is_ascii() {
        return Err("corpus must be nonempty bounded ASCII source".into());
    }
    let corpus = String::from_utf8(bytes)?;
    if corpus
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word.len() > 4096)
    {
        return Err("source token exceeds bounded chunk extension".into());
    }
    let identity = crc32fast::hash(corpus.as_bytes());
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut output = io::BufWriter::new(io::stdout().lock());
    writeln!(
        output,
        "{}",
        json!({"type":"ready", "role":args[0], "corpus_crc32":identity,
        "bytes":corpus.len(), "pid":std::process::id(), "kernel_mutations":0,
        "limits":{"address_space_bytes":getrlimit(Resource::RLIMIT_AS)?.1,
        "cpu_seconds":getrlimit(Resource::RLIMIT_CPU)?.1,"wall_seconds":60}})
    )?;
    output.flush()?;
    let mut offset = 0;
    let mut pass = 0;
    let mut searches = 0;
    let mut index: BTreeMap<String, u64> = BTreeMap::new();
    // Corpus size bounds chunks; command count independently bounds repeated work.
    for sequence in 0..16384 {
        let mut line = String::new();
        let n = std::io::Read::take(&mut input, 32).read_line(&mut line)?;
        if n == 0 || line == "finish\n" || line == "cancel\n" {
            drop(index);
            drop(corpus);
            writeln!(
                output,
                "{}",
                json!({"type":"final","searches":searches,"passes":pass,
                "partial_index_bytes":offset,"released":true,"cancelled":line != "finish\n"})
            )?;
            output.flush()?;
            return Ok(());
        }
        if line != "work\n" {
            return Err("invalid bounded command".into());
        }
        let started = Instant::now();
        let result = if args[0] == "search" {
            if searches >= 1024 {
                return Err("search request bound".into());
            }
            let term = TERMS[searches % TERMS.len()];
            let matches = corpus.lines().filter(|line| line.contains(term)).count();
            searches += 1;
            json!({"term":term,"matching_lines":matches,"searches":searches})
        } else {
            if pass >= 16 {
                return Err("index pass bound".into());
            }
            // Extend to a token boundary so chunked and whole-corpus indexing agree.
            let mut end = (offset + CHUNK).min(corpus.len());
            while end < corpus.len() && corpus.as_bytes()[end].is_ascii_alphanumeric() {
                end += 1;
            }
            for word in corpus[offset..end].split(|c: char| !c.is_ascii_alphanumeric()) {
                if !word.is_empty() {
                    *index.entry(word.to_ascii_lowercase()).or_default() += 1;
                }
            }
            offset = end;
            let done = offset == corpus.len();
            let mut digest = None;
            let mut words = None;
            if done {
                let mut hash = crc32fast::Hasher::new();
                for (word, count) in &index {
                    hash.update(word.as_bytes());
                    hash.update(&[0]);
                    hash.update(&count.to_le_bytes());
                }
                digest = Some(hash.finalize());
                words = Some(index.len());
                index.clear();
                offset = 0;
                pass += 1;
            }
            json!({"pass_complete":done,"passes":pass,"offset":offset,
                "index_crc32":digest,"unique_words":words})
        };
        writeln!(
            output,
            "{}",
            json!({"type":"result","sequence":sequence,
            "role":args[0],"service_ns":started.elapsed().as_nanos(),"result":result})
        )?;
        output.flush()?;
    }
    Err("command bound exceeded".into())
}
