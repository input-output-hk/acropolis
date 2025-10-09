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

---
