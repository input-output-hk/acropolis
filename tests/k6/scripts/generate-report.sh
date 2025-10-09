#!/bin/bash

# Generate HTML reports from JSON results
# Requires: npm install -g k6-html-reporter
if [ ! -d "results" ]; then
    echo "No results directory found!"
    exit 1
fi

echo "Generating HTML reports from JSON results..."

for json_file in results/*.json; do
    if [ -f "$json_file" ]; then
        html_file="${json_file%.json}.html"
        echo "Processing: $json_file -> $html_file"
        k6-reporter "$json_file" --output "$html_file"
    fi
done

echo "✅ Reports generated in results/ directory"