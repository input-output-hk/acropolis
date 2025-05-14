json_files = [f for f in glob.glob('governance-dir/00*.json')]

for j in json_files:
    print(j)