import matplotlib.pyplot as plt
import sys
import os
import numpy as np

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

def plot_latencies(latencies, output_file=None, log_scale=False):
    plt.figure(figsize=(10, 6))

    plt.ylim(0, max(latencies) * 1.1)

    plt.plot(latencies, label='Latency (ns)', color='b', linewidth=0.3)

    plt.xlabel('Sample')
    plt.ylabel('Latency (nanoseconds)')
    plt.title('Latency over Time')
    plt.legend()
    plt.grid(True)

    if log_scale:
        plt.yscale('log')
        plt.ylabel('Latency (nanoseconds, log scale)')
    
    elif not log_scale:
        plt.gca().yaxis.set_major_formatter(plt.FuncFormatter(lambda x, _: f'{int(x):d}'))

    if output_file:
        plt.savefig(output_file, dpi=300)  
        print(f"Plot saved to {output_file}")
    else:
        plt.show()

def plot_latencies_stem(latencies, output_file=None, log_scale=False):
    plt.figure(figsize=(10, 6))

    plt.ylim(0, max(latencies) * 1.1)  

    plt.stem(range(len(latencies)), latencies, linefmt='b-', markerfmt='bo', basefmt='k-')
    plt.xlabel('Sample')
    plt.ylabel('Latency (nanoseconds)')
    plt.title('Latency over Time')
    plt.grid(True)

    if log_scale:
        if any(latency <= 0 for latency in latencies):
            print("Warning: Logarithmic scale cannot be used with zero or negative values. Plotting with linear scale.")
        else:
            plt.yscale('log')
            plt.ylabel('Latency (nanoseconds, log scale)')
    
    if not log_scale:
        plt.gca().yaxis.set_major_formatter(plt.FuncFormatter(lambda x, _: f'{int(x):d}'))

    if output_file:
        plt.savefig(output_file, dpi=300)  
        print(f"Plot saved to {output_file}")
    else:
        plt.show()

if __name__ == '__main__':
    if len(sys.argv) != 3:
        print("Usage: python plot_latencies.py <path_to_latencies_file> <log_scale>")
        print("<log_scale> should be 'true' or 'false'")
        sys.exit(1)

    file_path = sys.argv[1]
    log_scale = sys.argv[2].lower() == 'true'

    if not os.path.exists(file_path):
        print(f"Error: File '{file_path}' does not exist.")
        sys.exit(1)

    latencies = read_latencies(file_path)

    file_name = os.path.basename(file_path)
    name, extension = file_name.rsplit(".", 1)
    output_file = name + '.png'

    save = True
    if save:
        plot_latencies(latencies, output_file, log_scale)
    else:
        plot_latencies(latencies, log_scale=log_scale)

