use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use rattler_build::script::CrLfNormalizer;
use std::hint::black_box;
use tokio_util::bytes::BytesMut;
use tokio_util::codec::Decoder;

fn normalize_line_breaks(c: &mut Criterion) {
    let mut group = c.benchmark_group("line_breaks");
    group.sample_size(10000);

    // Test 1: Realistic command output with carriage returns
    let command_output = [
        "pixi is based\r\n",
        "rattler is based\r\n",
        "conda-forge is based\r\n",
        "pixi build is based\r\n",
        "rattler-build is based\r\n",
        "prefix.dev is based\r\n",
    ]
    .join("");
    let large_command_output = command_output.repeat(100); // Make it larger for stable benchmarking

    group.throughput(Throughput::Bytes(large_command_output.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("command_output", large_command_output.len()),
        &large_command_output,
        |b, s| {
            b.iter(|| {
                let mut normalizer = CrLfNormalizer::default();
                let mut buffer = BytesMut::from(black_box(s.as_bytes()));
                normalizer.decode(&mut buffer).unwrap();
            });
        },
    );

    // Test 2: Build script output with mixed line breaks
    let build_script_output = [
        "line 1\r",
        "line 2\r",
        "line 3\r",
        "line 4\r",
        "line 5\r",
        "line 6\r",
        "line 7\r",
        "line 8\r",
        "line 9\r",
        "line 10\r",
        "done\n",
    ]
    .join("");
    let large_build_script_output = build_script_output.repeat(100);

    group.throughput(Throughput::Bytes(large_build_script_output.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("build_script_output", large_build_script_output.len()),
        &large_build_script_output,
        |b, s| {
            b.iter(|| {
                let mut normalizer = CrLfNormalizer::default();
                let mut buffer = BytesMut::from(black_box(s.as_bytes()));
                normalizer.decode(&mut buffer).unwrap();
            });
        },
    );

    // Test 3: Simulated chunked process output (as seen in run_process_with_replacements)
    let process_output = "This is line 1\r\nThis is line 2\r\nThis is line 3\r\n".repeat(50);
    let chunks: Vec<&[u8]> = process_output.as_bytes().chunks(16).collect();

    group.throughput(Throughput::Bytes(process_output.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("chunked_process_output", chunks.len()),
        &chunks,
        |b, chunks| {
            b.iter(|| {
                let mut normalizer = CrLfNormalizer::default();
                for &chunk in chunks.iter() {
                    let mut buffer = BytesMut::from(black_box(chunk));
                    normalizer.decode(&mut buffer).unwrap();
                }
            });
        },
    );

    // Test 4: Handling split CRLF sequence across chunks
    let part1 = "This is a test line ending with CR\r";
    let part2 = "\nThis is the next line";
    let large_test = (0..1000)
        .map(|i| if i % 2 == 0 { part1 } else { part2 })
        .collect::<String>();

    let chunks: Vec<&[u8]> = large_test.as_bytes().chunks(part1.len()).collect();

    group.throughput(Throughput::Bytes(large_test.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("split_crlf_chunks", chunks.len()),
        &chunks,
        |b, chunks| {
            b.iter(|| {
                let mut normalizer = CrLfNormalizer::default();
                for &chunk in chunks.iter() {
                    let mut buffer = BytesMut::from(black_box(chunk));
                    normalizer.decode(&mut buffer).unwrap();
                }
            });
        },
    );

    group.finish();
}

criterion_group!(benches, normalize_line_breaks);
criterion_main!(benches);
