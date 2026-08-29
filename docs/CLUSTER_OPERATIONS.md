# Cluster Operations Guide

## Overview
Pertisk now supports **multi-node clustering** with automatic failover (HA), live migration, and quorum-based consistency. This is **Phase 5** of the project.

## Quick Start: 3-Node Cluster

### 1. Start First Node (Node A)
```bash
# Initialize first node in solo mode
pertiskd --node-name "node-a"
# Will be leader automatically (1 out of 1 nodes = quorum)
```

### 2. Add Second Node (Node B)
```bash
# Start node B first
pertiskd --node-name "node-b" &

# Join it to node A
pertisk cluster join \
  --peer http://node-a:7480 \
  --username admin \
  --password admin
```

### 3. Add Third Node (Node C)
```bash
# Start node C
pertiskd --node-name "node-c" &

# Join it to any member
pertisk cluster join \
  --peer http://node-b:7480 \
  --username admin \
  --password admin
```

### 4. Verify Cluster
```bash
pertisk cluster status

# Output:
# cluster pertisk gen 10 leader node-a quorum true fenced false
# ID                  NAME     ONLINE   VCPU    MEM MiB  URL
# <node-a-id>         node-a   true     8/32    2048/16384  http://node-a:7480
# <node-b-id>         node-b   true     4/32    1024/16384  http://node-b:7480
# <node-c-id>         node-c   true     6/32    1536/16384  http://node-c:7480
```

## How Quorum Works

### Requirements
- **Majority vote**: `online_nodes * 2 > total_nodes`
- With 3 nodes: need 2 online → can lose 1 node
- With 5 nodes: need 3 online → can lose 2 nodes
- With 2 nodes: need 2 online → CANNOT lose any (no quorum with split-brain)

### Fencing
When a node loses quorum:
1. All running VMs on that node are **stopped** (fenced)
2. When quorum is restored, **HA-enabled VMs are restarted** elsewhere
3. This prevents split-brain scenarios

### Example: Node Failure
```
Initial state: 3 nodes, all online (quorum = YES)
  - VM1, VM2, VM3 running (all HA-enabled)

Node A goes offline:
  - Node B & C still online (2 out of 3 = quorum = YES)
  - Node B & C can accept operations
  - VM1 & VM2 & VM3 restart on B or C automatically

Node B goes offline too:
  - Only Node C online (1 out of 3 = quorum = NO)
  - Node C is FENCED (all VMs stopped)
  - No operations allowed until quorum restored

Nodes A & B come back:
  - Now 3 online again (quorum = YES)
  - Node C unfenced
  - Any HA VMs restart
```

## Cluster Operations

### View Status
```bash
pertisk cluster status
```

Shows:
- Cluster name and generation
- Leader node ID
- Quorum status
- Fenced status
- All members with CPU/RAM usage

### Join Cluster
```bash
pertisk cluster join \
  --peer http://existing-member:7480 \
  --username admin \
  --password admin
```

Requires:
- Authentication (username/password)
- Network connectivity to existing member
- Member accepts the join request

### Leave Cluster
```bash
pertisk cluster leave
```

- Reverts node to solo mode
- Clears cluster membership
- VMs stay local

## VM HA (High Availability)

### Enabling HA
```bash
# On create
pertisk vm create --id 100 ... # HA enabled by default

# On update
pertisk vm update 100 --ha true
```

### How HA Works
1. If a VM's owner node goes offline
2. Daemon checks cluster state
3. If quorum is maintained, **reschedule VM to healthy node**
4. Choose node based on:
   - Volume affinity (prefer nodes with VM's disks)
   - Least-loaded node (CPU + memory)
   - Network connectivity

### Disabling HA
```bash
pertisk vm update 100 --ha false
```

HA-disabled VMs won't restart on node failure. Good for:
- Testing
- Single-node deployments
- Stateless workloads that can be manually migrated

## Live Migration

### Migrate VM to Specific Node
```bash
pertisk vm migrate 100 --target <node-id>

# Or let scheduler pick best node
pertisk vm migrate 100
```

### What Happens
1. VM must be **running**
2. Source node initiates VMM live migration
3. Memory + state transferred to destination
4. Network connections preserved (if using TAP)
5. Ownership updated in cluster inventory

### Limitations
- Both nodes must have quorum
- Destination must have capacity
- Requires compatible VMM on both (e.g., Cloud Hypervisor)

## Scheduler Behavior

### Placement Algorithm
When creating or restarting a VM:

1. **Filter**: Find nodes with capacity
   - CPU: `available_vcpus >= vm.vcpus` (with 8x overcommit)
   - Memory: `available_memory >= vm.memory_mib` (hard cap)
   - Status: Node must be online

2. **Score**: Least-loaded node wins
   - CPU load: `(used_vcpus / total_vcpus) * 1000`
   - Memory load: `(used_memory / total_memory) * 1000`
   - Combined: `cpu_load + mem_load + node_id` (tie-breaker)

3. **Affinity**: For HA restarts
   - Prefer nodes that already have VM's volumes (Ceph/RBD)
   - Fall back to least-loaded if no affinity match

### Example
```
3 nodes, each 8 vCPU, 16GB RAM
Node A: 2 vCPU used, 4GB RAM used → load = 250 + 250 = 500
Node B: 4 vCPU used, 8GB RAM used → load = 500 + 500 = 1000
Node C: 1 vCPU used, 2GB RAM used → load = 125 + 125 = 250 ← WINNER

New VM (2 vCPU, 2GB) → placed on Node C
```

## Cluster Configuration

### Config File
`~/.pertisk/config.toml`
```toml
[cluster]
name = "production"           # Cluster name
node_name = "node-a"         # This node's name
peer_url = "http://x.x.x.x"  # Advertise URL
heartbeat_ms = 1000          # Heartbeat interval
offline_after_ms = 5000      # Timeout for node offline detection
cpus = 32                     # Override auto-detected CPUs
memory_mib = 131072          # Override auto-detected RAM (128GB)

[cluster]
join = "http://seed-node:7480"  # Auto-join on startup
```

### Environment Variables
```bash
PERTISK_NODE_NAME=node-a        # Node name
PERTISK_CLUSTER_HEARTBEAT_MS=1000
PERTISK_CLUSTER_OFFLINE_AFTER_MS=5000
```

## Troubleshooting

### Node Shows Offline
- Check network connectivity: `ping node-url`
- Check daemon is running: `curl http://node:7480/v1/host`
- Check firewall: port 7480 must be open
- Increase `offline_after_ms` if network is flaky

### Fenced and Can't Recover
```bash
# Forced cluster reset (DANGEROUS - data loss possible)
rm ~/.pertisk/state/cluster.json
pertiskd --node-name "node-a"  # Becomes solo again
```

### Can't Join Cluster
- **"Peer unreachable"**: Check peer URL and network
- **"Authentication failed"**: Wrong username/password
- **"Generation conflict"**: Network partition recovery; try again

### VMs Not Restarting on Failure
- Check `pertisk vm show <id>` → `ha: true`
- Check cluster quorum with `pertisk cluster status`
- Check volume affinity with `pertisk vol show <volume-id>`

## Best Practices

### Production Setup
1. **Minimum 3 nodes** for HA (can tolerate 1 failure)
2. **Dedicated cluster network** if possible
3. **Monitor quorum**: Alert if `quorum = false`
4. **Test failover** regularly in staging
5. **Enable HA** for critical VMs
6. **Distributed storage** (Ceph RBD) for VM mobility

### Testing
```bash
# Simulate node failure
node="node-b"
ssh $node "pkill pertiskd"

# Watch cluster detect failure
watch -n 1 'pertisk cluster status'

# Verify VM restarts
watch -n 1 'pertisk vm list'

# Bring node back
ssh $node "pertiskd --node-name node-b &"
```

### Monitoring
```bash
# Export cluster metrics (Prometheus-compatible)
curl http://localhost:7480/metrics | grep pertisk_cluster

# Key metrics:
# - pertisk_cluster_quorum
# - pertisk_cluster_members
# - pertisk_cluster_nodes_online
```

## Limitations & Future Work

### Current
- No arbitrator for 2-node clusters
- No automatic node replacement
- No persistent storage clustering (volumes stored locally)

### Coming (Phase 6)
- Distributed storage via Ceph RBD
- Cross-node volume replication
- Automatic volume sync

## See Also
- [GRAPHICS_CONSOLE.md](GRAPHICS_CONSOLE.md) - VGA console for ISO installers
- [Phase guide](2-phases.txt) - Project roadmap
