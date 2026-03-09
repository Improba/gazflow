use std::collections::HashMap;
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_steady_state(_c: &mut Criterion) {
    // Placeholder — sera enrichi avec un réseau GasLib réel.
    _c.bench_function("steady_state_2_nodes", |b| {
        b.iter(|| {
            // TODO: instancier un réseau et appeler solve_steady_state
        });
    });
}

criterion_group!(benches, bench_steady_state);
criterion_main!(benches);
