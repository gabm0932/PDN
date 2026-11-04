//! Performance benchmarks for pn-next

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use pn_next::runtime::{
    CreateContainerRequest, CreateSandboxRequest, ExecRequest, Runtime, RuntimeConfig,
    SandboxConfig, StartContainerRequest,
};
use proot_distro_core::ContainerConfig;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Build a sandbox (pod) configuration for benchmarking.
fn sandbox_config() -> SandboxConfig {
    SandboxConfig {
        hostname: "bench-sandbox".to_string(),
        dns_config: Default::default(),
        port_mappings: Vec::new(),
        labels: HashMap::new(),
        annotations: HashMap::new(),
    }
}

fn bench_sandbox_creation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let runtime = rt
        .block_on(async { Runtime::new(RuntimeConfig::default()) })
        .unwrap();

    let mut group = c.benchmark_group("sandbox_creation");
    group.measurement_time(Duration::from_secs(30));
    group.sample_size(10);

    group.bench_function("create_sandbox", |b| {
        b.to_async(&rt).iter(|| async {
            let sandbox = runtime
                .create_sandbox(black_box(CreateSandboxRequest {
                    config: sandbox_config(),
                    runtime_handler: None,
                }))
                .await
                .unwrap();
            black_box(sandbox);
        });
    });

    group.finish();
}

fn bench_container_creation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let runtime = rt
        .block_on(async { Runtime::new(RuntimeConfig::default()) })
        .unwrap();

    let mut group = c.benchmark_group("container_creation");
    group.measurement_time(Duration::from_secs(30));
    group.sample_size(10);

    group.bench_function("create_container", |b| {
        b.to_async(&rt).iter(|| async {
            runtime
                .create_sandbox(CreateSandboxRequest {
                    config: sandbox_config(),
                    runtime_handler: None,
                })
                .await
                .unwrap();

            let container = runtime
                .create_container(CreateContainerRequest {
                    config: ContainerConfig {
                        name: "bench-container".to_string(),
                        distro: "alpine".to_string(),
                        version: "latest".to_string(),
                        working_dir: Some("/root".to_string()),
                        hostname: Some("bench-container".to_string()),
                        ..Default::default()
                    },
                    sandbox_config: Some(sandbox_config()),
                    runtime_handler: None,
                })
                .await
                .unwrap();
            black_box(container);
        });
    });

    group.finish();
}

fn bench_exec_throughput(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let runtime = Arc::new(
        rt.block_on(async { Runtime::new(RuntimeConfig::default()) })
            .unwrap(),
    );

    rt.block_on(runtime.create_sandbox(CreateSandboxRequest {
        config: sandbox_config(),
        runtime_handler: None,
    }))
    .unwrap();

    let container_id = rt
        .block_on(runtime.create_container(CreateContainerRequest {
            config: ContainerConfig {
                name: "bench-container".to_string(),
                distro: "alpine".to_string(),
                version: "latest".to_string(),
                working_dir: Some("/root".to_string()),
                hostname: Some("bench-container".to_string()),
                ..Default::default()
            },
            sandbox_config: Some(sandbox_config()),
            runtime_handler: None,
        }))
        .unwrap();

    // Bring the container up before measuring exec throughput. Starting a
    // container initialises the PROot engine (proot binary + host binds),
    // which is not available in every environment; exec_container only needs
    // the container registered in the runtime, so a failed start must not
    // abort the benchmark fixture.
    let _ = rt.block_on(runtime.start_container(StartContainerRequest {
        container_id: container_id.clone(),
    }));

    let mut group = c.benchmark_group("exec_throughput");
    group.measurement_time(Duration::from_secs(30));
    group.sample_size(20);

    for size in [1, 10, 100].iter() {
        group.throughput(Throughput::Elements(*size));
        group.bench_with_input(BenchmarkId::new("exec_batch", size), size, |b, &size| {
            b.to_async(&rt).iter(|| async {
                let mut handles = Vec::new();
                for _ in 0..size {
                    let runtime = runtime.clone();
                    let container_id = container_id.clone();
                    handles.push(tokio::spawn(async move {
                        runtime
                            .exec_container(ExecRequest {
                                container_id,
                                command: vec!["echo".to_string(), "hello".to_string()],
                                env: Vec::new(),
                                working_dir: "/root".to_string(),
                                user: "root".to_string(),
                                tty: false,
                                stdin: false,
                                stdout: true,
                                stderr: true,
                            })
                            .await
                    }));
                }
                for handle in handles {
                    let _ = handle.await;
                }
            });
        });
    }

    group.finish();
}

fn bench_guardrails_evaluation(c: &mut Criterion) {
    use pn_gateway::{GuardrailConfig, GuardrailManager};

    let rt = tokio::runtime::Runtime::new().unwrap();
    let guardrails = GuardrailManager::new(GuardrailConfig::default());

    let mut group = c.benchmark_group("guardrails_evaluation");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    group.bench_function("evaluate_request", |b| {
        b.to_async(&rt).iter(|| async {
            let result = guardrails.check_all();
            let _ = black_box(result);
        });
    });

    group.finish();
}

fn bench_fail_safe_circuit_breaker(c: &mut Criterion) {
    use pn_gateway::{FailSafeConfig, FailSafeManager};

    let rt = tokio::runtime::Runtime::new().unwrap();
    let fail_safe = FailSafeManager::new(FailSafeConfig::default());

    let mut group = c.benchmark_group("fail_safe_circuit_breaker");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    group.bench_function("execute_with_circuit_breaker", |b| {
        b.to_async(&rt).iter(|| async {
            let result = fail_safe
                .execute_with_failsafe(|| async { Ok::<_, anyhow::Error>("success".to_string()) })
                .await;
            let _ = black_box(result);
        });
    });

    group.finish();
}

fn bench_seccomp_filter_generation(c: &mut Criterion) {
    use proot_compat::security::SecurityHardeningManager;

    let mut group = c.benchmark_group("seccomp_filter_generation");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    group.bench_function("generate_default_profile", |b| {
        b.iter(|| {
            let profile = SecurityHardeningManager::generate_default_seccomp_profile();
            black_box(profile);
        });
    });

    group.bench_function("generate_restricted_profile", |b| {
        b.iter(|| {
            let profile = SecurityHardeningManager::generate_restricted_seccomp_profile();
            black_box(profile);
        });
    });

    group.finish();
}

fn bench_path_translation(c: &mut Criterion) {
    use proot_compat::path_translation::{translate_guest_to_host, translate_host_to_guest};
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    let temp_dir = tempdir().unwrap();
    let host_root = temp_dir.path().to_path_buf();
    let guest_root = PathBuf::from("/guest");

    let paths = vec![
        Path::new("/simple/path"),
        Path::new("/deep/nested/path/with/many/components"),
        Path::new("/path/with/symlink"),
    ];

    let mut group = c.benchmark_group("path_translation");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    group.bench_function("translate_to_host", |b| {
        b.iter(|| {
            for path in &paths {
                let _ = translate_guest_to_host(black_box(*path), &guest_root, &host_root);
            }
        });
    });

    group.bench_function("translate_to_guest", |b| {
        b.iter(|| {
            for path in &paths {
                let _ = translate_host_to_guest(black_box(*path), &host_root, &guest_root);
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_sandbox_creation,
    bench_container_creation,
    bench_exec_throughput,
    bench_guardrails_evaluation,
    bench_fail_safe_circuit_breaker,
    bench_seccomp_filter_generation,
    bench_path_translation,
);
criterion_main!(benches);
