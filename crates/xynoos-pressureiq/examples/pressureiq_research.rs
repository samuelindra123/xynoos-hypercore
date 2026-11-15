use std::{hint::black_box, time::Instant};
use xynoos_pressureiq::{parse_meminfo, parse_psi, parse_vmstat};
fn main() {
    let cases=[("psi","some avg10=0.00 avg60=0.00 avg300=0.00 total=123\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=12\n"),("mem","MemTotal: 1234 kB\nMemAvailable: 1200 kB\n"),("vm","pgfault 123\npgscan_direct 5\n")];
    println!("case,iterations,total_ns,ns_per_parse");
    for (name, input) in cases {
        let start = Instant::now();
        for _ in 0..10000 {
            match name {
                "psi" => {
                    black_box(parse_psi(black_box(input)).unwrap());
                }
                "mem" => {
                    black_box(parse_meminfo(black_box(input)).unwrap());
                }
                _ => {
                    black_box(parse_vmstat(black_box(input)).unwrap());
                }
            }
        }
        let ns = start.elapsed().as_nanos();
        println!("{name},10000,{ns},{}", ns / 10000);
    }
}
