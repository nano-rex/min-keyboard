//! Measures the matrix kernels and one forward pass in CPU time of the calling thread, which
//! other programs on the machine do not disturb much:
//! `cargo run --release -p min-keyboard-grammar --example bench -- model.gguf`

use grammar::quant::{Kind, Unpacked, Q8};

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

extern "C" {
    fn clock_gettime(clock: i32, time: *mut Timespec) -> i32;
}

fn cpu_seconds() -> f64 {
    const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
    let mut t = Timespec { sec: 0, nsec: 0 };
    // SAFETY: writes into the struct above.
    unsafe { clock_gettime(CLOCK_THREAD_CPUTIME_ID, &mut t) };
    t.sec as f64 + t.nsec as f64 * 1e-9
}

/// The best of five runs of `f`, in CPU seconds.
fn best(mut f: impl FnMut()) -> f64 {
    (0..5)
        .map(|_| {
            let start = cpu_seconds();
            f();
            cpu_seconds() - start
        })
        .fold(f64::MAX, f64::min)
}

fn main() {
    let columns = 1024;
    let x: Vec<f32> = (0..columns).map(|i| (i as f32).sin()).collect();
    let q = Q8::quantize(&x);
    for kind in [Kind::Q4K, Kind::Q6K, Kind::Q8_0] {
        let row: Vec<u8> = (0..kind.row_bytes(columns)).map(|i| (i * 7 % 251) as u8).collect();
        let mut u = Unpacked::default();
        let n = 20_000;
        let unpack = best(|| (0..n).for_each(|_| u.unpack(kind, &row, columns)));
        let mut sink = 0.0;
        let dot = best(|| (0..n).for_each(|_| sink += u.dot(&q, &x)));
        println!("{kind:?}: unpack {:.3} us/row, dot {:.3} us/row ({})", unpack * 1e6 / n as f64, dot * 1e6 / n as f64, sink > 0.0);
    }
    let Some(path) = std::env::args().nth(1) else { return };
    let model = grammar::model::Model::load(std::path::Path::new(&path)).unwrap();
    let pool = grammar::pool::Pool::new(1);
    let prompt: Vec<u32> = (0..64).map(|i| 1000 + i).collect();
    let prefill = best(|| {
        let mut cache = model.new_cache();
        model.forward(&pool, &mut cache, &prompt);
    });
    let mut cache = model.new_cache();
    model.forward(&pool, &mut cache, &prompt);
    let decode = best(|| {
        model.forward(&pool, &mut cache, &[100]);
    });
    println!("one thread: prefill {:.1} ms/token, decode {:.1} ms/token", prefill * 1000.0 / 64.0, decode * 1000.0);
}
