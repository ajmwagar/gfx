#![allow(missing_docs)]
//! CPU-side scene construction throughput.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use fpl_gfx::{Primitive, Rect, Scene};

fn build_dense_scene(c: &mut Criterion) {
    c.bench_function("build 1024 control primitives", |bench| {
        let mut scene = Scene::with_capacity(1_024);
        bench.iter(|| {
            scene.clear();
            for index in 0_u16..1_024 {
                let x = f32::from(index % 32) * 24.0;
                let y = f32::from(index / 32) * 24.0;
                scene.push(Primitive::knob(
                    Rect::new(x, y, 20.0, 20.0),
                    f32::from(index % 100) / 99.0,
                ));
            }
            black_box(&scene);
        });
    });
}

criterion_group!(benches, build_dense_scene);
criterion_main!(benches);
