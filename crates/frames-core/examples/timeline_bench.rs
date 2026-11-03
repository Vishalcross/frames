//! Times the timeline query on a library (frames_swift_spec.md 7.1: under
//! 50 ms for 100k items). Usage: cargo run --release --example timeline_bench -- <frames.sqlite3>

use std::time::Instant;

use frames_core::store::{SearchFilter, Store};

fn main() {
    let path = std::env::args().nth(1).expect("usage: timeline_bench <frames.sqlite3>");
    let start = Instant::now();
    let store = Store::open(&path).expect("open the database");
    println!("open: {:.1} ms", start.elapsed().as_secs_f64() * 1000.0);
    for _ in 0..3 {
        let start = Instant::now();
        let mut count = 0;
        store
            .for_each_timeline_tile(&SearchFilter::default(), |_| count += 1)
            .expect("timeline");
        println!("timeline: {count} tiles in {:.1} ms", start.elapsed().as_secs_f64() * 1000.0);
    }
}
