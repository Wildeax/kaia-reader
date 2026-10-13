//! Looks up words in every dictionary under a folder.
//! Usage: cargo run -p kaia-dict --example lookup -- <dict dir> <word>...
fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("dictionary folder");
    let lib = kaia_dict::Library::open_dir(std::path::Path::new(&dir)).expect("open dictionaries");
    for (path, err) in lib.errors() {
        eprintln!("skipped {}: {err}", path.display());
    }
    for word in args {
        let entries = lib.lookup(&word);
        println!("== {word}: {} entries", entries.len());
        for e in entries.iter().take(3) {
            let text = e.definition.plain_text();
            let first: String = text.lines().next().unwrap_or("").chars().take(70).collect();
            println!("  [{}] {} ({:?}): {}", e.dict_name, e.headword, e.matched, first);
        }
    }
}
