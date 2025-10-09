.PHONY: help install build test smoke load stress reports clean

.DEFAULT_GOAL := help

API_URL ?= http://127.0.0.1:4340

help:
	@echo "Acropolis K6 Performance Tests"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Available targets:"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-15s %s\n", $$1, $$2}'

install: ## Install dependencies
	@echo "Installing dependencies..."
	npm install

build: ## Build TypeScript files
	@echo "Building TypeScript..."
	npm run build

results-dir: ## Create results directory
	@echo "Creating results directory..."
	@mkdir -p results
	@echo "✅ Results directory created"

smoke: build results-dir ## Run smoke test only (1 minute)
	@echo "Running Smoke Test..."
	@export API_URL=$(API_URL) && k6 run --out json=results/smoke-$$(date +%Y%m%d-%H%M%S).json dist/smoke.test.js

load: build results-dir ## Run load test only (16 minutes)
	@echo "Running Load Test..."
	@export API_URL=$(API_URL) && k6 run --out json=results/load-$$(date +%Y%m%d-%H%M%S).json dist/load.test.js

stress: build results-dir ## Run stress test only (13 minutes)
	@echo "Running Stress Test..."
	@export API_URL=$(API_URL) && k6 run --out json=results/stress-$$(date +%Y%m%d-%H%M%S).json dist/stress.test.js

test: ## Run all performance tests (30 minutes total)
	@chmod +x ./run-tests.sh
	@API_URL=$(API_URL) ./run-tests.sh

reports: ## Generate HTML reports from JSON results
	@chmod +x ./generate-reports.sh
	@./generate-reports.sh

clean: ## Remove results directory
	@echo "Cleaning results..."
	rm -rf results/
	@echo "✅ Results cleaned"

clean-build: ## Remove build artifacts
	@echo "Cleaning build artifacts..."
	rm -rf dist/
	@echo "✅ Build artifacts cleaned"

clean-all: clean clean-build ## Remove all generated files
	@echo "✅ All generated files cleaned"