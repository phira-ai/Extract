# Sync

Extract stores everything under `.extract/`, so stores can move between laptop, workstation, and HPC machines.

## Push to remote

```bash
extract sync push user@hpc:/path/.extract/
```

Uploads local `.extract/` via `rsync`.

## Pull from remote

```bash
extract sync pull user@hpc:/path/.extract/
```

Downloads and merges remote data into local store.

## Archive

```bash
extract sync export backup.tar.gz
```

Creates a compressed archive of the local store.

## Import archive

```bash
extract sync import backup.tar.gz
```

Imports archive contents into local `.extract/` and merges database rows.

## Merge behavior

- Experiments match by hierarchy path.
- Runs use ULIDs, so run IDs should not collide across machines.
- Pull/import report merged experiment and run counts.
- Existing data is kept when already up to date.

## Recommended HPC workflow

On laptop:

```bash
extract sync push user@hpc:/scratch/project/.extract/
```

On HPC job, log normally with Python SDK.

Back on laptop:

```bash
extract sync pull user@hpc:/scratch/project/.extract/
extract tui
```
