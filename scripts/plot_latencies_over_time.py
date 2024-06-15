import matplotlib.pyplot as plt
import sys
import os

def read_latencies(file_path):
    """
    Reads latencies from a text file. Each line in the file should contain a single latency value.
    :param file_path: Path to the text file.
    :return: List of latency values.
    """
    latencies = []
    with open(file_path, 'r') as file:
        for line in file:
            try:
                latencies.append(int(line.strip()))
            except ValueError:
                print(f"Warning: Could not convert line to integer: {line.strip()}")
    return latencies

def plot_latencies(latencies):
    plt.figure(figsize=(10, 6))
    plt.plot(latencies, label='Latency (ns)', color='b')
    plt.xlabel('Sample')
    plt.ylabel('Latency (nanoseconds)')
    plt.title('Latency over Time')
    plt.legend()
    plt.grid(True)
    plt.show()

def plot_latencies_save(latencies, output_file):
    plt.figure(figsize=(10, 6))
    plt.plot(latencies, label='Latency (ns)', color='b')
    plt.xlabel('Sample')
    plt.ylabel('Latency (nanoseconds)')
    plt.title('Latency over Time')
    plt.legend()
    plt.grid(True)
    plt.savefig(output_file)
    print(f"Plot saved to {output_file}")

if __name__ == '__main__':

    if len(sys.argv) != 2:
        print("Usage: python plot_latencies.py <path_to_latencies_file>")
        sys.exit(1)

    file_path = sys.argv[1]

    if not os.path.exists(file_path):
        print(f"Error: File '{file_path}' does not exist.")
        sys.exit(1)

    latencies = read_latencies(file_path)

    output_file = 'latency_plot.png'

    save = True;
    if save:
        plot_latencies_save(latencies, output_file)
    else: 
        plot_latencies(latencies)

