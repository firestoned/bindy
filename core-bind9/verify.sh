#!/bin/bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT

# Bind9 Verification and Testing Script

NAMESPACE="bind9"
POD_NAME=$(kubectl get pods -n $NAMESPACE -l app=bind9 -o jsonpath='{.items[0].metadata.name}')

if [ -z "$POD_NAME" ]; then
    echo "❌ No Bind9 pods found in namespace $NAMESPACE"
    exit 1
fi

echo "🔍 Bind9 Cluster Verification"
echo "=============================="
echo ""

# Test 1: Pod status
echo "1️⃣  Pod Status:"
kubectl get pods -n $NAMESPACE -l app=bind9 -o wide
echo ""

# Test 2: Service status
echo "2️⃣  Service Status:"
kubectl get svc -n $NAMESPACE
echo ""

# Test 3: DNS resolution
echo "3️⃣  DNS Resolution Test:"
echo "   Testing localhost resolution..."
kubectl exec -n $NAMESPACE "$POD_NAME" -- dig @127.0.0.1 localhost +short
echo ""

# Test 4: RNDC status
echo "4️⃣  RNDC Status:"
kubectl exec -n $NAMESPACE "$POD_NAME" -- rndc -s 127.0.0.1 -p 953 status 2>/dev/null || \
    echo "   ⚠️  RNDC status command needs configuration update"
echo ""

# Test 5: Log summary
echo "5️⃣  Recent Logs (last 10 lines):"
kubectl logs -n $NAMESPACE "$POD_NAME" --tail=10
echo ""

# Test 6: PVC status
echo "6️⃣  Storage Status:"
kubectl get pvc -n $NAMESPACE
echo ""

# Test 7: Node availability
echo "7️⃣  NodePort Availability:"
NODES=$(kubectl get nodes -o jsonpath='{.items[*].status.addresses[?(@.type=="ExternalIP")].address}')
if [ -z "$NODES" ]; then
    NODES=$(kubectl get nodes -o jsonpath='{.items[*].status.addresses[?(@.type=="InternalIP")].address}')
fi

if [ -z "$NODES" ]; then
    echo "   ⚠️  Could not determine cluster IPs"
else
    echo "   Cluster nodes: $NODES"
    echo "   NodePort: 30053"
    echo "   Example: dig @<NODE_IP> -p 30053 localhost"
fi
echo ""

# Test 8: Linkerd injection
echo "8️⃣  Linkerd Injection Status:"
kubectl get pods -n $NAMESPACE -l app=bind9 -o jsonpath='{.items[*].metadata.annotations.linkerd\.io/inject}' | grep -q "enabled" && \
    echo "   ✅ Linkerd injection enabled" || \
    echo "   ⚠️  Linkerd injection not detected"
echo ""

echo "=============================="
echo "✨ Verification complete!"
