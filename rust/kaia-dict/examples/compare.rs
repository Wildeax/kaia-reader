//! Prints headwords for comparing a dictionary folder with sdcv.
//! Usage: cargo run -p kaia-dict --example compare -- DIR exact|fuzzy WORD...
use kaia_dict::{Library, LookupOptions};

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("dictionary folder");
    let mode = args.next().expect("exact or fuzzy");
    let library = Library::open_dir(dir).expect("open dictionaries");
    let options = LookupOptions {
        fuzzy: mode == "fuzzy",
        ..LookupOptions::default()
    };
    for word in args {
        let headwords: Vec<_> = library
            .lookup_with(&word, &options)
            .into_iter()
            .map(|entry| format!("{}:{}", entry.dict_name, entry.headword))
            .collect();
        println!("{}\t{}", word, headwords.join("|"));
    }
}
