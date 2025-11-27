
# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

NAMESPACE="dns-system"
CLUSTER_NAME="bindy-test"

echo -e "${BLUE}🧪 Testing Bindy Controller${NC}"
echo ""

# Check if cluster exists
if ! kind get clusters 2>/dev/null | grep -q "^${CLUSTER_NAME}$"; then
    echo -e "${RED}❌ Cluster '${CLUSTER_NAME}' not found. Run ./deploy/kind-deploy.sh first${NC}"
    exit 1
fi

# Set context
kubectl config use-context "kind-${CLUSTER_NAME}" > /dev/null

echo -e "${GREEN}1️⃣  Checking controller status...${NC}"
if kubectl get deployment bindy-controller -n "${NAMESPACE}" &>/dev/null; then
    kubectl get pods -n "${NAMESPACE}" -l app=bindy-controller
    echo -e "${GREEN}✅ Controller is running${NC}"
else
    echo -e "${RED}❌ Controller not found${NC}"
    exit 1
fi
echo ""

echo -e "${GREEN}2️⃣  Checking CRDs...${NC}"
CRDS=("dnszones" "bind9instances" "arecords" "aaaarecords" "txtrecords" "cnamerecords" "mxrecords" "nsrecords" "srvrecords" "caarecords")
for crd in "${CRDS[@]}"; do
    if kubectl get crd "${crd}.dns.example.com" &>/dev/null; then
        echo -e "  ${GREEN}✓${NC} ${crd}.dns.example.com"
    else
        echo -e "  ${RED}✗${NC} ${crd}.dns.example.com"
    fi
done
echo ""

echo -e "${GREEN}3️⃣  Applying test resources...${NC}"

# Create a test Bind9Instance
echo -e "${YELLOW}Creating Bind9Instance...${NC}"
kubectl apply -f - <<EOF
apiVersion: dns.example.com/v1alpha1
kind: Bind9Instance
metadata:
  name: test-bind9
  namespace: ${NAMESPACE}
  labels:
    environment: test
    role: primary
spec:
  replicas: 1
  version: "9.18"
EOF

# Wait a moment for the instance to be created
sleep 2

# Create a test zone
echo -e "${YELLOW}Creating DNSZone...${NC}"
kubectl apply -f - <<EOF
apiVersion: dns.example.com/v1alpha1
kind: DNSZone
metadata:
  name: test-zone
  namespace: ${NAMESPACE}
spec:
  zoneName: test.local
  type: primary
  instanceSelector:
    matchLabels:
      environment: test
  soaRecord:
    primaryNS: ns1.test.local.
    adminEmail: admin@test.local
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
  ttl: 3600
EOF

# Wait for zone to be created
sleep 3

# Create test A record
echo -e "${YELLOW}Creating ARecord...${NC}"
kubectl apply -f - <<EOF
apiVersion: dns.example.com/v1alpha1
kind: ARecord
metadata:
  name: test-www
  namespace: ${NAMESPACE}
spec:
  zone: test-zone
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
EOF

# Create test TXT record
echo -e "${YELLOW}Creating TXTRecord...${NC}"
kubectl apply -f - <<EOF
apiVersion: dns.example.com/v1alpha1
kind: TXTRecord
metadata:
  name: test-txt
  namespace: ${NAMESPACE}
spec:
  zone: test-zone
  name: "@"
  text:
    - "v=spf1 mx ~all"
  ttl: 3600
EOF

echo ""
echo -e "${GREEN}4️⃣  Waiting for reconciliation (10 seconds)...${NC}"
sleep 10

echo ""
echo -e "${GREEN}5️⃣  Checking resource status...${NC}"
echo -e "${BLUE}Bind9Instances:${NC}"
kubectl get bind9instances -n "${NAMESPACE}" -o wide

echo ""
echo -e "${BLUE}DNSZones:${NC}"
kubectl get dnszones -n "${NAMESPACE}" -o wide

echo ""
echo -e "${BLUE}ARecords:${NC}"
kubectl get arecords -n "${NAMESPACE}" -o wide

echo ""
echo -e "${BLUE}TXTRecords:${NC}"
kubectl get txtrecords -n "${NAMESPACE}" -o wide

echo ""
echo -e "${GREEN}6️⃣  Checking controller logs (last 20 lines)...${NC}"
kubectl logs -n "${NAMESPACE}" -l app=bindy-controller --tail=20

echo ""
echo -e "${GREEN}✅ Test complete!${NC}"
echo ""
echo -e "${YELLOW}📋 Next Steps:${NC}"
echo "1. Check resource details:"
echo "   kubectl describe dnszone test-zone -n ${NAMESPACE}"
echo ""
echo "2. Watch controller logs:"
echo "   kubectl logs -n ${NAMESPACE} -l app=bindy-controller -f"
echo ""
echo "3. Clean up test resources:"
echo "   kubectl delete arecords,txtrecords,dnszones,bind9instances --all -n ${NAMESPACE}"
echo ""
echo "4. Destroy the cluster:"
echo "   ./deploy/kind-cleanup.sh"
