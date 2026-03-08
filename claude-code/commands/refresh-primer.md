Refresh the code-primer file summaries for this project. This re-summarizes only changed, new, or deleted files using content hashing.

Run the following command:

```bash
code-primer refresh $ARGUMENTS .
```

After the command completes:
1. Read the JSON report from stdout — it lists what changed
2. Read the updated `code-primer.json` from the output directory
3. Use the refreshed summaries for the rest of this conversation — disregard any earlier version loaded at session start
