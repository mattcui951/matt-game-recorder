//! Prints what the Outplayed importer reads from Outplayed's database.
fn main() {
    println!("{}", mgr_lib::import::debug_counts());
    println!("{}", mgr_lib::import::outplayed_summary());
}
