import sys
import numpy as np

def filter_and_sample(lat, cdf, threshold=1.0, sparse_frac=0.1):
    dense_indices = cdf > threshold

    sparse_indices = ~dense_indices
    sparse_sample_size = int(np.sum(sparse_indices) * sparse_frac)
    sparse_sample_indices = np.random.choice(
        np.where(sparse_indices)[0], size=sparse_sample_size, replace=False
    )

    # Combine indices and sort
    final_indices = np.sort(
        np.concatenate((np.where(dense_indices)[0], sparse_sample_indices))
    )
    return lat[final_indices], cdf[final_indices]


def read_log_file(file_path):
    latencies = []
    with open(file_path, "r") as file:
        for line in file:
            parts = line.strip().split(",")
            latencies.append(float(parts[1]))
    return np.array(latencies)


if __name__ == "__main__":
    input = sys.argv[1]

    if input.endswith(".log"):
        latencies = read_log_file(input)
    else:
        latencies = np.loadtxt(input)

    print("Min. latency:", np.min(latencies))
    print("Median latency:", np.median(latencies))
    print("Average latency:", np.mean(latencies))
    print("90th percentile:", np.percentile(latencies, 90))
    print("99th percentile:", np.percentile(latencies, 99))
    print("99.99th percentile:", np.percentile(latencies, 99.99))

    lat, counts = np.unique(latencies, return_counts=True)
    cdf = np.cumsum(counts)
    cdf = cdf / cdf[-1]

    # play around with sparse_frac
    lat, cdf = filter_and_sample(lat, cdf, sparse_frac=2000 / len(cdf))

    # "nanoseconds -> microseconds (might not be necessary with fio logs)
    lat = lat / 1000

    output = sys.argv[2]
    np.savetxt(
        output,
        np.vstack((lat, cdf)).T,
        delimiter=",",
        header="latency,cdf",
        comments="",
        fmt=["%.4f", "%s"],
    )
