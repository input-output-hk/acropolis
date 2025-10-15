#!/bin/bash

set -e

echo "=========================================="
echo "Running Acropolis Performance Tests"
echo "=========================================="
export API_URL=${API_URL:-"http://127.0.0.1:4340"}

echo "Testing API at: $API_URL"
echo ""

if [ ! -d "results" ]; then
    echo "Creating results directory..."
    mkdir -p results
    echo "✅ Results directory created"
    echo ""
fi

echo "Building TypeScript..."
npm run build
echo ""

echo "=========================================="
echo "1. Running Smoke Test (1 minute)"
echo "=========================================="
K6_WEB_DASHBOARD=true K6_WEB_DASHBOARD_EXPORT=results/smoke-$(date +%Y%m%d-%H%M%S).html k6 run --out json=results/smoke-$(date +%Y%m%d-%H%M%S).json dist/smoke.test.js

if [ $? -ne 0 ]; then
    echo "❌ Smoke test failed! Stopping test suite."
    exit 1
fi

echo "✅ Smoke test passed!"
echo ""

echo "=========================================="
echo "2. Running Load Test (16 minutes)"
echo "=========================================="
K6_WEB_DASHBOARD=true K6_WEB_DASHBOARD_EXPORT=results/load-$(date +%Y%m%d-%H%M%S).html k6 run --out json=results/load-$(date +%Y%m%d-%H%M%S).json dist/load.test.js

if [ $? -ne 0 ]; then
    echo "⚠️  Load test failed, but continuing..."
else
    echo "✅ Load test passed!"
fi
echo ""

echo "=========================================="
echo "3. Running Stress Test (13 minutes)"
echo "=========================================="
K6_WEB_DASHBOARD=true K6_WEB_DASHBOARD_EXPORT=results/stress-$(date +%Y%m%d-%H%M%S).html k6 run --out json=results/stress-$(date +%Y%m%d-%H%M%S).json dist/stress.test.js

if [ $? -ne 0 ]; then
    echo "⚠️  Stress test failed (expected behavior)"
else
    echo "✅ Stress test passed!"
fi
echo ""

echo "=========================================="
echo "All tests completed!"
echo "Results saved in results/ directory"
echo "=========================================="