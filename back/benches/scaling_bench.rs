use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use gazflow_back::graph::{ConnectionKind, GasNetwork, Node, Pipe};
use gazflow_back::solver::solve_steady_state;
use std::collections::HashMap;

fn build_chain_network(node_count: usize) -> GasNetwork {
    let mut net = GasNetwork::new();
    for i in 0..node_count {
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
    for i in 0..(node_count.saturating_sub(1)) {
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

fn bench_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("scaling");
    for node_count in [11_usize, 24, 40, 135, 582, 4197] {
        let network = build_chain_network(node_count);
        let mut demands = HashMap::new();
        demands.insert(format!("N{}", node_count - 1), -3.0);
        group.bench_with_input(
            BenchmarkId::new("newton_chain_nodes", node_count),
            &node_count,
            |b, _| {
                b.iter(|| {
                    let result = solve_steady_state(&network, &demands, 2000, 5e-4).expect("solve");
                    black_box(result.residual);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_scaling);
criterion_main!(benches);
