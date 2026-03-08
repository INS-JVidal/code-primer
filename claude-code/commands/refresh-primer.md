Refresh the code-primer file summaries for this project. This re-summarizes only changed, new, or deleted files using content hashing.

Run the following command:

```bash
code-primer --refresh $ARGUMENTS .
```

After the command completes:
1. Read the updated `code-primer.json` from the output directory
2. Report what changed: new summaries, updated summaries, removed files
3. Use the refreshed summaries for the rest of this conversation — disregard any earlier version loaded at session start
