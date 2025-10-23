# Deploy Queue

A deployment orchestration system that manages and coordinates deployments across cloud providers, regions, cells, and environments using PostgreSQL as a backend. This tool ensures safe, sequential deployments by preventing conflicting deployments from running simultaneously in the same cloud provider, region, and cell.

## Overview

Deploy Queue solves the problem of coordinating multiple concurrent deployments across different cloud providers, regions, cells, and environments. It acts as a centralized queue that prevents deployment conflicts by:

- **Blocking conflicting deployments**: Prevents multiple deployments in the same environment, cloud provider, region, and cell from running simultaneously
- **Automatic queueing**: Automatically waits for blocking deployments to complete before starting, ensuring deployments start in the order they entered the queue
- **Multi-dimensional isolation**: Deployments in different environment, cloud providers, regions, or cells can run in parallel
- **Buffer time management**: Enforces cooldown periods after deployments (configurable per environment)
- **Concurrency key support**: Allows specific deployments to run in parallel when they share a concurrency key
- **State tracking**: Tracks deployment lifecycle (queued → running → finished/cancelled)

## How It Works

### Deployment States

A deployment goes through these states:

1. **Queued** - Deployment is created but waiting for blockers to clear
2. **Running** - Deployment has started and is actively deploying
3. **Buffering** - Deployment has finished but is still within the buffer time
4. **Finished** - Deployment completed successfully
5. **Cancelled** - Deployment was cancelled before or during execution

```mermaid
stateDiagram-v2
    [*] --> Queued
    Queued --> Running
    Queued --> Canceled
    Running --> Buffering
    Running --> Canceled
    Buffering --> Finished
```

### Blocking Logic

A deployment is blocked by other deployments in the same cloud provider, region, and cell when:

1. ✅ The blocking deployment has a **smaller ID** (was queued earlier)
2. ✅ They are in the **same environment, cloud provider, region, and cell**
3. ✅ They have **different or no concurrency keys** (cannot run concurrently)
4. ✅ The blocker is either:
   - Still **running** (no finish timestamp), OR
   - **Finished within the buffer time** (e.g., within last 10 minutes for prod)
5. ✅ The blocker is **not cancelled**

**Example:**
```
Cloud Provider: aws, Region: us-west-2, Cell: 1, Environment: prod (10-minute buffer)

ID | Component | State    | Time
---+-----------+----------+------------
1  | api       | finished | 5 min ago  → BLOCKS new deployments
2  | web       | running  | -          → BLOCKS new deployments
3  | database  | queued   | -          → WAITING for #1 and #2
```

## Installation

### Prerequisites

- Rust 1.75+ (uses 2024 edition)
- PostgreSQL database
- Environment variable `DEPLOY_QUEUE_DATABASE_URL` set to your PostgreSQL connection string

### Building from Source

```bash
cd deploy-queue
cargo build --release
```

The binary will be available at `target/release/deploy-queue`.

### GitHub Action

This tool is designed to be used as a GitHub Action. See [action.yml](action.yml) for the GitHub Action interface.

## CLI Usage

The deploy-queue CLI has four main commands:

### 1. Start a Deployment

Queues a new deployment and waits for conflicting deployments to complete before starting.

```bash
deploy-queue start --environment <ENVIRONMENT> --provider <PROVIDER> --region <REGION> --cell-index <CELL_INDEX> --component <COMPONENT> [OPTIONS]
```

**Required Flags:**
- `--environment <ENVIRONMENT>` - Target environment: `dev` or `prod`
- `--provider <PROVIDER>` - Cloud provider (e.g., `aws`, `azure`, `gcp`)
- `--region <REGION>` - Target region (e.g., `us-west-2`, `eu-central-1`)
- `--cell-index <CELL_INDEX>` - Cell index to deploy (e.g., `0`, `1`, `2`)
- `--component <COMPONENT>` - Component being deployed (e.g., `api`, `web`, `database`)

**Optional Flags:**
- `--version <VERSION>` - Version of the component
- `--url <URL>` - Link to the GitHub Actions job or deployment info
- `--note <NOTE>` - Additional notes for manual deployments
- `--concurrency-key <KEY>` - Allow parallel deployments with same key

**Example:**
```bash
deploy-queue start --environment prod --provider aws --region us-west-2 --cell-index 1 --component api \
  --version "v1.2.3" \
  --url "https://github.com/org/repo/actions/runs/123" \
  --note "Hotfix for critical bug"
```

**Output:**
- Prints the deployment ID
- Writes `deployment-id=<ID>` to `$GITHUB_OUTPUT` if running in GitHub Actions
- Blocks until all conflicting deployments complete
- Starts the deployment automatically when ready

### 2. Finish a Deployment

Mark a deployment as finished.

```bash
deploy-queue finish <DEPLOYMENT_ID>
```

**Example:**
```bash
deploy-queue finish 42
```

### 3. Cancel a Deployment

Cancel a deployment with an optional note.

```bash
deploy-queue cancel <DEPLOYMENT_ID> [CANCELLATION_NOTE]
```

**Example:**
```bash
deploy-queue cancel 42 "Cancelled due to failing health checks"
```

### 4. Get Deployment Info

Get information about a specific deployment.

```bash
deploy-queue info <DEPLOYMENT_ID>
```

**Example:**
```bash
deploy-queue info 42
```

**Output:**
```
42 deployed api@v1.2.3: (Hotfix for critical bug) (https://github.com/org/repo/actions/runs/123)
```

## Configuration

### Environment Variables

- `DEPLOY_QUEUE_DATABASE_URL` (required) - PostgreSQL connection string
  ```bash
  export DEPLOY_QUEUE_DATABASE_URL="postgresql://user:password@localhost/deploy_queue"
  ```

- `DEPLOY_QUEUE_LOG_LEVEL` (optional) - Log level (default: `info`)
  ```bash
  export DEPLOY_QUEUE_LOG_LEVEL=debug
  ```

### Environment Settings

Buffer times are configured in the database:

| Environment | Buffer Time | Purpose |
|-------------|-------------|---------|
| `dev`       | 0 minutes   | Fast iteration, no waiting |
| `prod`      | 10 minutes  | Safety buffer between deployments |

These values are set in the initial migration and can be adjusted in the `environments` table.

## Database Schema

The system uses two main tables:

### `deployments`

Stores all deployment records with metadata and timestamps.

| Column | Type | Description |
|--------|------|-------------|
| `id` | BIGSERIAL | Auto-incrementing primary key |
| `environment` | VARCHAR(50) | Target environment (dev/prod) |
| `cloud_provider` | VARCHAR(100) | Cloud provider (aws, azure, gcp, etc.) |
| `region` | VARCHAR(100) | Target region |
| `cell_index` | INTEGER | Cell index |
| `component` | VARCHAR(200) | Component being deployed |
| `version` | VARCHAR(100) | Version identifier (optional) |
| `url` | TEXT | Link to deployment job (optional) |
| `note` | TEXT | Deployment notes (optional) |
| `concurrency_key` | VARCHAR(100) | Concurrency key (optional) |
| `start_timestamp` | TIMESTAMPTZ | When deployment started |
| `finish_timestamp` | TIMESTAMPTZ | When deployment finished |
| `cancellation_timestamp` | TIMESTAMPTZ | When deployment was cancelled |
| `cancellation_note` | TEXT | Reason for cancellation |
| `created_at` | TIMESTAMPTZ | When record was created |
| `updated_at` | TIMESTAMPTZ | Last update time |

### `environments`

Configures buffer times for each environment.

| Column | Type | Description |
|--------|------|-------------|
| `environment` | VARCHAR(50) | Environment name (PRIMARY KEY) |
| `buffer_time` | INTEGER | Buffer time in minutes |


## Usage in GitHub Actions

### Basic Example

```yaml
name: Deploy API
on:
  push:
    branches: [main]

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - name: Start deployment
        id: deploy-queue-start
        uses: neondatabase/dev-actions/deploy-queue@v1
        with:
          mode: start
          environment: prod
          cloud-provider: aws
          region: us-west-2
          cell-index: 1
          component: api
          version: ${{ github.sha }}

      - name: Run actual deployment
        run: |
          # Your deployment commands here
          echo "Deploying..."
          
      - name: Finish deployment
        if: success()
        uses: neondatabase/dev-actions/deploy-queue@v1
        with:
          mode: finish
          deployment-id: ${{ steps.deploy-queue-start.outputs.deployment-id }}

      - name: Cancel deployment
        if: failure()
        uses: neondatabase/dev-actions/deploy-queue@v1
        with:
          mode: cancel
          deployment-id: ${{ steps.deploy-queue-start.outputs.deployment-id }}
          cancellation-note: "Deployment failed"
```

### Parallel Deployments with Concurrency Key

```yaml
- name: Start hotfix deployment
  uses: neondatabase/dev-actions/deploy-queue@v1
  with:
    mode: start
    environment: prod
    cloud-provider: aws
    region: us-west-2
    cell-index: 1
    component: api
    version: v1.2.4
    concurrency-key: hotfix-2024-001  # Same key = can run in parallel
```

### Breaking Glass: Emergency Deployments

For emergency situations where you need to bypass the deploy queue entirely, use a repository variable to conditionally enable/disable the queue:

```yaml
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - name: Start deployment (with queue)
        id: deploy-queue-start
        if: ${{ vars.DEPLOY_QUEUE_ENABLED == 'true' }}
        uses: neondatabase/dev-actions/deploy-queue@v1
        with:
          mode: start
          environment: prod
          cloud-provider: aws
          region: us-west-2
          cell-index: 1
          component: api
          version: v1.2.3
          url: https://github.com/org/repo/actions/runs/123

      - name: Run actual deployment
        run: |
          # Your deployment commands here
          echo "Deploying..."
          
      - name: Finish deployment (with queue)
        if: ${{ success() && vars.DEPLOY_QUEUE_ENABLED == 'true' }}
        uses: neondatabase/dev-actions/deploy-queue@v1
        with:
          mode: finish
          deployment-id: ${{ steps.deploy-queue-start.outputs.deployment-id }}

      - name: Cancel deployment (with queue)
        if: ${{ !success() && vars.DEPLOY_QUEUE_ENABLED == 'true' }}
        uses: neondatabase/dev-actions/deploy-queue@v1
        with:
          mode: cancel
          deployment-id: ${{ steps.deploy-queue-start.outputs.deployment-id }}
          cancellation-note: "Deployment failed"
```

**To bypass the queue in emergencies:**
1. Set the repository variable `DEPLOY_QUEUE_ENABLED` to `false`
2. Trigger the deployment - it will skip all queue operations
3. After the emergency, set `DEPLOY_QUEUE_ENABLED` back to `true`

This "breaking glass" mechanism allows you to maintain deployment velocity during critical incidents while keeping the safety of the queue for normal operations.

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test suite
cargo test --test blocking_deployments_tests

# Run integration tests
cargo test --test integration_tests
```

See [SAMPLE_DATA.md](SAMPLE_DATA.md) for information about test data and comprehensive test coverage.

### Database Migrations

Migrations are automatically applied when the CLI runs. Migration files are in `migrations/`:

- `0001_initial_deployments_schema.sql` - Initial schema with deployments and environments tables
- `0002_add_cloud_provider_and_cell_index.sql` - Adds cloud_provider and cell_index fields to deployments table


To run migrations manually:

```rust
use deploy_queue::{create_db_connection, run_migrations};

let pool = create_db_connection().await?;
run_migrations(&pool).await?;
```

### Maintaining the .sqlx Cache

This project uses sqlx's offline mode for compile-time checked SQL queries. The prepared query metadata is stored in `.sqlx/` (gitignored) to enable compilation without a database connection in CI/CD environments.

**When to update the cache:**

You need to regenerate the .sqlx cache whenever you:
- Add, modify, or remove SQL queries in the code
- Change the database schema (via migrations)
- Update sqlx dependencies

**Regenerating the cache:**

```bash
# Ensure your database is running and migrations are applied
export DEPLOY_QUEUE_DATABASE_URL="postgresql://deploy_queue:deploy_queue@localhost/deploy_queue"

# Regenerate the cache
cargo sqlx prepare

# Or use the workspace flag if running from repository root
cargo sqlx prepare --workspace
```

**Troubleshooting:**

If you encounter compile errors about query metadata:

```bash
# Clean the cache and regenerate
rm -rf .sqlx
cargo sqlx prepare
```

If compilation fails in CI without a database, ensure the `.sqlx/` directory is committed to the repository (it should be for this project).

### Local Development

1. Start a local PostgreSQL instance:
   ```bash
   docker run -d \
     -e POSTGRES_DB=deploy_queue \
     -e POSTGRES_USER=deploy_queue \
     -e POSTGRES_PASSWORD=deploy_queue \
     -p 5432:5432 \
     postgres:16
   ```

2. Set environment variable:
   ```bash
   export DEPLOY_QUEUE_DATABASE_URL="postgresql://deploy_queue:deploy_queue@localhost/deploy_queue"
   ```

3. Run the CLI:
   ```bash
   cargo run -- start --environment dev --provider aws --region us-west-2 --cell-index 1 --component api --version v1.0.0
   ```

## Architecture

### Components

- **CLI (`src/cli.rs`)** - Command-line argument parsing using `clap`
- **Library (`src/lib.rs`)** - Core logic for deployment management
- **Queries (`queries/`)** - SQL queries for blocking detection
- **Migrations (`migrations/`)** - Database schema versioning
- **Tests (`tests/`)** - Integration and unit tests

## Queries

### Finding Blocking Deployments

The core query (`queries/blocking_deployments.sql`) finds deployments blocking a specific deployment. 
It consists of the following steps:

1. Get the target deployment's environment, cloud provider, region and cell
2. Find all deployments in the same environment, cloud provider, region, and cell with earlier IDs
3. Filter for different/null concurrency keys
4. Exclude finished (outside buffer) and cancelled deployments

## License

See [LICENSE](../LICENSE) at the repository root.

## Contributing

This tool is part of the [neondatabase/dev-actions](https://github.com/neondatabase/dev-actions) repository. Please see the main repository for contribution guidelines.

