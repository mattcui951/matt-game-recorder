//! Prints Outplayed's raw match records for one champion: outplayed_info <leveldb dir> <champion>
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(dir), Some(champ)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: outplayed_info <leveldb dir> <champion>");
        std::process::exit(2);
    };
    print!("{}", mgr_lib::import::debug_match_info(std::path::Path::new(dir), champ));
}
