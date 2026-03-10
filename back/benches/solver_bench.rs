use criterion::{Criterion, black_box, criterion_group, criterion_main};
use gazflow_back::gaslib::{load_network, load_scenario_demands};
use gazflow_back::graph::{ConnectionKind, GasNetwork, Node, Pipe};
use gazflow_back::solver::{solve_steady_state, solve_steady_state_jacobi};
use rayon::ThreadPoolBuilder;
use std::collections::HashMap;
use std::path::Path;

fn build_chain_network(pipe_count: usize) -> GasNetwork {
    let mut net = GasNetwork::new();
    for i in 0..=pipe_count {
        net.add_node(Node {
            id: format!("N{i}"),
            x: i as f64,
            y: 0.0,
            lon: None,
            lat: None,
            height_m: 0.0,
            pressure_lower_bar: None,
            pressure_upper_bar: None,
            pressure_fixed_bar: if i == 0 { Some(70.0) } else { None },
        });
    }
    for i in 0..pipe_count {
        net.add_pipe(Pipe {
            id: format!("P{i}"),
            from: format!("N{i}"),
            to: format!("N{}", i + 1),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 5.0,
            diameter_mm: 500.0,
            roughness_mm: 0.05,
        });
    }
    net
}

fn bench_steady_state(c: &mut Criterion) {
    let net = build_chain_network(80);
    let mut demands = HashMap::new();
    demands.insert("N80".to_string(), -3.0);

    let pool_one = ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("rayon pool(1)");
    let par_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8)
        .max(2);
    let pool_many = ThreadPoolBuilder::new()
        .num_threads(par_threads)
        .build()
        .expect("rayon pool(N)");

    c.bench_function("steady_state_newton_parallel_1_thread", |b| {
        b.iter(|| {
            let result = pool_one
                .install(|| solve_steady_state(&net, &demands, 1500, 5e-4))
                .expect("newton solve");
            black_box(result.residual);
        });
    });

    c.bench_function("steady_state_newton_parallel_n_threads", |b| {
        b.iter(|| {
            let result = pool_many
                .install(|| solve_steady_state(&net, &demands, 1500, 5e-4))
                .expect("newton solve");
            black_box(result.residual);
        });
    });

    c.bench_function("steady_state_jacobi_baseline", |b| {
        b.iter(|| {
            // Jacobi peut échouer sur des cas "stiff"; on garde un benchmark robuste
            // qui ne panique pas en release bench.
            let residual = solve_steady_state_jacobi(&net, &demands, 5000, 5e-3)
                .map(|r| r.residual)
                .unwrap_or(f64::INFINITY);
            black_box(residual);
        });
    });

    let gaslib135_net = Path::new("dat/GasLib-135.net");
    let gaslib135_scn = Path::new("dat/GasLib-135.scn");
    if gaslib135_net.exists() && gaslib135_scn.exists() {
        let network = load_network(gaslib135_net).expect("load GasLib-135 network");
        let scenario = load_scenario_demands(gaslib135_scn).expect("load GasLib-135 scenario");
        c.bench_function("steady_state_newton_gaslib_135", |b| {
            b.iter(|| {
                // Tolérance bench volontairement un peu plus permissive pour éviter
                // les flakiness de non-convergence limite en profil release.
                let iters = solve_steady_state(&network, &scenario.demands, 3000, 1e-3)
                    .map(|r| r.iterations as f64)
                    .unwrap_or(f64::INFINITY);
                black_box(iters);
            });
        });
    }
}

criterion_group!(benches, bench_steady_state);
criterion_main!(benches);
