import time

started = time.perf_counter_ns()
total = 0
for value in range(100000):
    total += value
elapsed_ns = time.perf_counter_ns() - started
assert total == 4999950000
print("WASMDROID_BENCHMARK_OK", total, elapsed_ns)
