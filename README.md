# Kago

Kago (籠, Japanese for "basket/container") is a container orchestrator written in Rust. It provides Kubernetes-like declarative deployment management.

![kago image](./img/irasutoya_kago.png)

## How to Use

### 1. Start the control plane

```bash
kago serve --port 8080
```

### 2. Start worker node(s)

On each worker node:

```bash
kago agent --name worker-1 --master http://master-ip:8080 --port 8081
```

Options:
- `--name`, `-n`: Node name (required)
- `--master`, `-m`: Control plane server URL (default: `http://localhost:8080`)
- `--port`, `-p`: Port to listen on (default: 8081)
- `--address`, `-a`: Address to advertise to the control plane (defaults to hostname)
- `--cpu`: CPU capacity in millicores (default: 4000)
- `--memory`: Memory capacity in MB (default: 8192)

### 3. Deploy an Application

Create a deployment manifest (see `examples/nginx-deployment.yml`):

```yaml
kind: Deployment
spec:
  name: nginx
  image: nginx:alpine
  replicas: 3
  resources:
    cpu: 100m
    memory: 128Mi
```

Or you can define multiple deployments in a single file using `---` as a separator (see `examples/multi-deployment.yml`):

```yaml
kind: Deployment
spec:
  name: web
  image: nginx:alpine
  replicas: 2
  resources:
    cpu: 100m
    memory: 128Mi
---
kind: Deployment
spec:
  name: api
  image: httpd:alpine
  replicas: 2
  resources:
    cpu: 200m
    memory: 256Mi
```

Apply it:

```bash
kago apply -f examples/nginx-deployment.yml
```

### 4. Check Status

```bash
# List deployments
kago get deployments

# List pods
kago get pods

# List nodes
kago get nodes
```

## CLI Reference

### Server Commands

```bash
# Start control plane with default settings
kago serve

# Start control plane with custom port and scheduler
kago serve --port 8080 --scheduler best-fit
```

### Worker node Commands

```bash
# Start worker node with custom resources
kago agent --name worker-1 \
           --master http://localhost:8080 \
           --port 8081 \
           --cpu 2000 \
           --memory 4096
```

### Client Commands

```bash
# Apply deployment(s) from YAML file
kago apply -f deployment.yml

# Get resources
kago get deployments
kago get pods
kago get nodes

# Delete a deployment
kago delete <deployment-name>
```

## Scheduling Strategies

Kago supports multiple scheduling strategies that can be selected when starting the control plane:

| Strategy | Description |
|----------|-------------|
| `first-fit` | Selects the first node with sufficient resources |
| `best-fit` | Selects the node that will be most fully utilized |
| `least-allocated` | Selects the node with the most available resources |
| `balanced` | Balances CPU and memory utilization across nodes |

```bash
# Examples
kago serve --scheduler first-fit
kago serve --scheduler best-fit
kago serve --scheduler least-allocated
kago serve --scheduler balanced
```

## REST API

In addition to the CLI, you can interact with Kago directly via its REST API using tools like `curl` or any HTTP client.

### Example: Using curl

```bash
# Check health
curl http://localhost:8080/health

# Create a deployment
curl -X POST http://localhost:8080/deployments \
  -H "Content-Type: application/json" \
  -d '{"name": "nginx", "image": "nginx:alpine", "replicas": 3, "resources": {"cpu_millis": 100, "memory_mb": 128}}'

# List deployments
curl http://localhost:8080/deployments

# Scale a deployment
curl -X PUT http://localhost:8080/deployments/nginx \
  -H "Content-Type: application/json" \
  -d '{"name": "nginx", "image": "nginx:alpine", "replicas": 5, "resources": {"cpu_millis": 100, "memory_mb": 128}}'

# Delete a deployment
curl -X DELETE http://localhost:8080/deployments/nginx

# List pods
curl http://localhost:8080/pods

# List nodes
curl http://localhost:8080/nodes
```

## Running Tests

### Unit Tests

```bash
cargo test
```

### e2e Tests

```bash
# Basic tests with first-fit scheduler
./tests/e2e.sh

# Test specific scheduler strategy
./tests/e2e.sh --scheduler best-fit
./tests/e2e.sh --scheduler least-allocated

# Include multi-node tests
./tests/e2e.sh --scheduler least-allocated --multi-node

# Test all scheduler strategies
./tests/test-all-schedulers.sh
```

## License

MIT
