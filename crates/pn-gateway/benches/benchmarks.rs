//! Performance benchmarks for pn-gateway

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use pn_gateway::{
    config::{HealthCheck, HealthCheckType},
    FailSafeConfig, FailSafeManager, GuardrailConfig, GuardrailManager, HealthConfig,
    HealthMonitor, MetricsCollector, MetricsConfig,
};
use std::sync::Arc;
use std::time::Duration;

fn bench_guardrails_evaluation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let guardrails = GuardrailManager::new(GuardrailConfig::default());

    let mut group = c.benchmark_group("guardrails_evaluation");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    group.bench_function("evaluate_request", |b| {
        b.to_async(&rt).iter(|| async {
            let result = guardrails.check_all().unwrap();
            black_box(result);
        });
    });

    group.finish();
}

fn bench_guardrails_batch_evaluation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let guardrails = Arc::new(GuardrailManager::new(GuardrailConfig::default()));

    let mut group = c.benchmark_group("guardrails_batch_evaluation");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(20);

    for batch_size in [1, 10, 50, 100].iter() {
        group.throughput(Throughput::Elements(*batch_size));
        group.bench_with_input(
            BenchmarkId::new("batch_evaluate", batch_size),
            batch_size,
            |b, &size| {
                b.to_async(&rt).iter(|| async {
                    let mut handles = Vec::new();
                    for _ in 0..size {
                        let guardrails = guardrails.clone();
                        handles.push(tokio::spawn(async move { guardrails.check_all() }));
                    }
                    for handle in handles {
                        let _ = handle.await;
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_fail_safe_circuit_breaker(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let fail_safe = FailSafeManager::new(FailSafeConfig::default());

    let mut group = c.benchmark_group("fail_safe_circuit_breaker");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    group.bench_function("execute_success", |b| {
        b.to_async(&rt).iter(|| async {
            let result = fail_safe
                .execute_with_failsafe(|| async { Ok::<_, anyhow::Error>("success".to_string()) })
                .await;
            let _ = black_box(result);
        });
    });

    group.bench_function("execute_failure", |b| {
        b.to_async(&rt).iter(|| async {
            let result = fail_safe
                .execute_with_failsafe(|| async {
                    Err::<String, _>(anyhow::anyhow!("simulated failure"))
                })
                .await;
            let _ = black_box(result);
        });
    });

    group.finish();
}

fn bench_fail_safe_retry_policy(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let fail_safe = FailSafeManager::new(FailSafeConfig {
        max_retries: 3,
        retry_delay: Duration::from_millis(10),
        ..Default::default()
    });

    let mut group = c.benchmark_group("fail_safe_retry_policy");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(20);

    group.bench_function("retry_on_failure", |b| {
        b.to_async(&rt).iter(|| async {
            let attempt = std::sync::atomic::AtomicUsize::new(0);
            let result = fail_safe
                .execute_with_failsafe(|| async {
                    let current = attempt.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if current < 2 {
                        Err(anyhow::anyhow!("timeout"))
                    } else {
                        Ok("success".to_string())
                    }
                })
                .await;
            let _ = black_box(result);
        });
    });

    group.finish();
}

fn bench_fail_safe_bulkhead(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let fail_safe = Arc::new(FailSafeManager::new(FailSafeConfig::default()));

    let mut group = c.benchmark_group("fail_safe_bulkhead");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(20);

    for concurrent in [1, 5, 10, 20].iter() {
        group.throughput(Throughput::Elements(*concurrent));
        group.bench_with_input(
            BenchmarkId::new("bulkhead_concurrent", concurrent),
            concurrent,
            |b, &size| {
                b.to_async(&rt).iter(|| async {
                    let mut handles = Vec::new();
                    for _ in 0..size {
                        let fail_safe = fail_safe.clone();
                        handles.push(tokio::spawn(async move {
                            fail_safe
                                .execute_with_failsafe(|| async {
                                    tokio::time::sleep(Duration::from_millis(10)).await;
                                    Ok::<String, anyhow::Error>("done".to_string())
                                })
                                .await
                        }));
                    }
                    for handle in handles {
                        let _ = handle.await;
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_health_monitor(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let make_check = || HealthCheck {
        name: "test-check".to_string(),
        check_type: HealthCheckType::MemoryUsage,
        target: "system".to_string(),
        interval: Duration::from_secs(30),
        timeout: Duration::from_secs(5),
    };

    let health = HealthMonitor::new(HealthConfig {
        checks: vec![make_check()],
        ..Default::default()
    });

    let mut group = c.benchmark_group("health_monitor");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    group.bench_function("register_check", |b| {
        b.iter(|| {
            let monitor = HealthMonitor::new(HealthConfig {
                checks: vec![black_box(make_check())],
                ..Default::default()
            });
            black_box(monitor);
        });
    });

    group.bench_function("check_health", |b| {
        b.to_async(&rt).iter(|| async {
            let result = health.run_check("test-check").await.unwrap();
            black_box(result);
        });
    });

    group.finish();
}

fn bench_metrics_collection(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let metrics = MetricsCollector::new(MetricsConfig::default());

    let mut group = c.benchmark_group("metrics_collection");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    group.bench_function("record_counter", |b| {
        b.to_async(&rt).iter(|| async {
            metrics.increment_counter(black_box("test_counter"), black_box(1));
        });
    });

    group.bench_function("record_histogram", |b| {
        b.to_async(&rt).iter(|| async {
            metrics.record_histogram(black_box("test_histogram"), black_box(123.456));
        });
    });

    group.bench_function("record_gauge", |b| {
        b.to_async(&rt).iter(|| async {
            metrics.set_gauge(black_box("test_gauge"), black_box(42.0));
        });
    });

    group.bench_function("export_metrics", |b| {
        b.to_async(&rt).iter(|| async {
            let result = metrics.get_metrics().await;
            black_box(result);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_guardrails_evaluation,
    bench_guardrails_batch_evaluation,
    bench_fail_safe_circuit_breaker,
    bench_fail_safe_retry_policy,
    bench_fail_safe_bulkhead,
    bench_health_monitor,
    bench_metrics_collection,
);
criterion_main!(benches);
