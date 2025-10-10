# Acropolis K6s Performance Tests

Performance testing suite for the Acropolis Cardano Node leveraging [Grafana's k6 tool](https://github.com/grafana/k6).

## Prerequisites

Before running the tests, you'll need to install Node.js dependencies and k6.

### Install Node Dependencies

```bash
npm install
```

## Installing k6

For more detailed instructions on installing k6, please refer to the [documentation here](https://grafana.com/docs/k6/latest/set-up/install-k6/#install-k6).


### macOS

Using Homebrew:

```bash
brew install k6
```

### Linux

#### Debian/Ubuntu

```bash
sudo gpg -k
sudo gpg --no-default-keyring --keyring /usr/share/keyrings/k6-archive-keyring.gpg \
  --keyserver hkp://keyserver.ubuntu.com:80 \
  --recv-keys C5AD17C747E3415A3642D57D77C6C491D6AC1D69

echo "deb [signed-by=/usr/share/keyrings/k6-archive-keyring.gpg] https://dl.k6.io/deb stable main" | \
  sudo tee /etc/apt/sources.list.d/k6.list

sudo apt-get update
sudo apt-get install k6
```

#### Fedora/CentOS

```bash
sudo dnf install https://dl.k6.io/rpm/repo.rpm
sudo dnf install k6
```

### Verify Installation

Confirm k6 is installed correctly:

```bash
k6 version
```

## Project Structure

```
performance-tests/
├── src/
│   ├── tests/        # Test configurations (smoke, load, stress, soak)
│   ├── scenarios/    # Endpoint-specific test scenarios
│   ├── config/       # Endpoints, test data, thresholds
│   └── utils/        # Helpers, metrics, checks
├── scripts/          # Run and report generation scripts
└── Makefile          # Convenient test commands
```

## Usage

See the `Makefile` for all available commands:

```bash
# Build TypeScript
make build

# Run individual tests
make test-smoke    # Quick validation (1 minute)
make test-load     # Sustained load (16 minutes)
make test-stress   # Find breaking point (13 minutes)
make test-soak     # Long-running stability (2+ hours)

# Run all tests
make test-all

# Clean build artifacts
make clean
```

### Environment Variables

Configure the API endpoint:

```bash
export API_URL="http://127.0.0.1:4340"
make test-smoke
```

Or inline:

```bash
API_URL="http://127.0.0.1:4340" make test-load
```

## Results

Test results are saved as JSON files in the `results/` directory with timestamps.

### Generate HTML Reports

```bash
# Install reporter
npm install -g k6-html-reporter

# Generate reports
./scripts/generate-report.sh
```

## Customization

### Modify Load Patterns

Edit test files in `src/tests/` to adjust:
- Virtual user counts
- Ramp-up/ramp-down durations
- Test duration
- Traffic distribution weights

### Add New Endpoints

1. Add endpoint definition to `src/config/endpoints.ts`
2. Add test data to `src/config/test-data.ts`
3. Create scenario function in `src/scenarios/`
4. Import and use in test files

### Adjust Thresholds

Edit `src/config/thresholds.ts` to modify performance expectations.


---
