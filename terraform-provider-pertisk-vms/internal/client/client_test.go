package client

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
)

func TestParseSize(t *testing.T) {
	got, err := ParseSize("10G")
	if err != nil {
		t.Fatal(err)
	}
	if got != 10*1024*1024*1024 {
		t.Fatalf("got %d", got)
	}
	got, err = ParseSize("512M")
	if err != nil {
		t.Fatal(err)
	}
	if got != 512*1024*1024 {
		t.Fatalf("got %d", got)
	}
	if FormatSize(32*1024*1024*1024) != "32G" {
		t.Fatalf("format: %s", FormatSize(32*1024*1024*1024))
	}
}

func TestFlexID(t *testing.T) {
	var id FlexID
	if err := json.Unmarshal([]byte("100"), &id); err != nil || id.String() != "100" {
		t.Fatalf("number: %v %q", err, id)
	}
	if err := json.Unmarshal([]byte(`"abc"`), &id); err != nil || id.String() != "abc" {
		t.Fatalf("string: %v %q", err, id)
	}
	raw, err := json.Marshal(FlexID("100"))
	if err != nil || string(raw) != "100" {
		t.Fatalf("marshal numeric: %v %s", err, raw)
	}
}

func TestClientCRUD(t *testing.T) {
	var token string
	vms := map[string]VM{}
	nets := map[string]Network{}
	vols := map[string]Volume{}

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		auth := strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")
		if r.URL.Path != "/v1/login" && auth != "tok-1" {
			w.WriteHeader(http.StatusUnauthorized)
			_ = json.NewEncoder(w).Encode(ErrorBody{Error: "unauthorized"})
			return
		}
		switch {
		case r.Method == http.MethodPost && r.URL.Path == "/v1/login":
			token = "tok-1"
			_ = json.NewEncoder(w).Encode(TokenResponse{Token: token, Username: "admin", Role: "admin"})
		case r.URL.Path == "/v1/session":
			_ = json.NewEncoder(w).Encode(Session{ID: "u1", Username: "admin", Role: "admin"})
		case r.URL.Path == "/v1/cluster":
			_ = json.NewEncoder(w).Encode(Cluster{Name: "pertisk", Generation: 1, Quorum: true, Members: []ClusterMember{{ID: "n1", Name: "n1", Online: true}}})
		case r.Method == http.MethodPost && r.URL.Path == "/v1/networks":
			net := Network{ID: "net-1", Name: "lan", CIDR: "10.90.0.0/24", Mode: "nat", DHCP: true, Isolate: true, Bridge: "br-lan"}
			nets[net.ID] = net
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(net)
		case r.Method == http.MethodGet && r.URL.Path == "/v1/networks/net-1":
			_ = json.NewEncoder(w).Encode(nets["net-1"])
		case r.Method == http.MethodDelete && r.URL.Path == "/v1/networks/net-1":
			delete(nets, "net-1")
			w.WriteHeader(http.StatusNoContent)
		case r.Method == http.MethodPost && r.URL.Path == "/v1/volumes":
			vol := Volume{ID: "vol-1", Name: "web-disk", Format: "qcow2", SizeBytes: 32 * 1024 * 1024 * 1024}
			vols[vol.ID] = vol
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(vol)
		case r.Method == http.MethodGet && r.URL.Path == "/v1/volumes/vol-1":
			_ = json.NewEncoder(w).Encode(vols["vol-1"])
		case r.Method == http.MethodPost && r.URL.Path == "/v1/vms":
			vm := VM{ID: "100", Spec: VMSpec{Name: "web", VCPUs: 2, MemoryMiB: 2048, HA: true}, State: "created"}
			vms[vm.ID.String()] = vm
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(vm)
		case r.Method == http.MethodPost && r.URL.Path == "/v1/vms/100/disks":
			vm := vms["100"]
			vm.Spec.Disks = append(vm.Spec.Disks, Disk{VolumeID: "vol-1"})
			vms["100"] = vm
			_ = json.NewEncoder(w).Encode(vm)
		case r.Method == http.MethodPost && r.URL.Path == "/v1/vms/100/nics":
			vm := vms["100"]
			vm.Spec.Nets = append(vm.Spec.Nets, Nic{NetworkID: "net-1"})
			vms["100"] = vm
			_ = json.NewEncoder(w).Encode(vm)
		case r.Method == http.MethodGet && r.URL.Path == "/v1/vms/100":
			vm, ok := vms["100"]
			if !ok {
				w.WriteHeader(http.StatusNotFound)
				_ = json.NewEncoder(w).Encode(ErrorBody{Error: "not found"})
				return
			}
			_ = json.NewEncoder(w).Encode(vm)
		case r.Method == http.MethodPatch && r.URL.Path == "/v1/vms/100":
			vm := vms["100"]
			vm.Spec.VCPUs = 4
			vms["100"] = vm
			_ = json.NewEncoder(w).Encode(vm)
		case r.Method == http.MethodPost && r.URL.Path == "/v1/vms/100/start":
			vm := vms["100"]
			vm.State = "running"
			vms["100"] = vm
			_ = json.NewEncoder(w).Encode(vm)
		case r.Method == http.MethodDelete && r.URL.Path == "/v1/vms/100":
			delete(vms, "100")
			w.WriteHeader(http.StatusNoContent)
		default:
			w.WriteHeader(http.StatusNotFound)
			_ = json.NewEncoder(w).Encode(ErrorBody{Error: "not found: " + r.URL.Path})
		}
	}))
	defer srv.Close()

	c, err := New(Config{Endpoint: srv.URL, Username: "admin", Password: "admin"})
	if err != nil {
		t.Fatal(err)
	}
	net, err := c.CreateNetwork(CreateNetworkRequest{Name: "lan", Mode: "nat"})
	if err != nil || net.ID != "net-1" {
		t.Fatalf("network: %v %+v", err, net)
	}
	vol, err := c.CreateVolume(CreateVolumeRequest{Name: "web-disk", SizeBytes: 32 * 1024 * 1024 * 1024, Format: "qcow2"})
	if err != nil || vol.ID != "vol-1" {
		t.Fatalf("volume: %v %+v", err, vol)
	}
	vm, err := c.CreateVM(CreateVMRequest{ID: uint64(100), Name: "web", VCPUs: 2, MemoryMiB: 2048, HA: true})
	if err != nil || vm.ID.String() != "100" {
		t.Fatalf("vm: %v %+v", err, vm)
	}
	if _, err := c.AttachDisk("100", vol.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := c.AttachNic("100", net.ID, ""); err != nil {
		t.Fatal(err)
	}
	cpus := 4
	if _, err := c.UpdateVM("100", UpdateVMRequest{VCPUs: &cpus}); err != nil {
		t.Fatal(err)
	}
	started, err := c.StartVM("100")
	if err != nil || started.State != "running" {
		t.Fatalf("start: %v %+v", err, started)
	}
	got, err := c.GetVM("100")
	if err != nil || len(got.Spec.Disks) != 1 || len(got.Spec.Nets) != 1 {
		t.Fatalf("get: %v %+v", err, got)
	}
	if err := c.DeleteVM("100"); err != nil {
		t.Fatal(err)
	}
	if _, err := c.GetVM("100"); !IsNotFound(err) {
		t.Fatalf("expected not found, got %v", err)
	}
}

func TestInferImageFormat(t *testing.T) {
	if InferImageFormat("ubuntu-24.04.img") != "qcow2" {
		t.Fatal("img")
	}
	if InferImageFormat("disk.qcow2") != "qcow2" {
		t.Fatal("qcow2")
	}
	if InferImageFormat("disk.raw") != "raw" {
		t.Fatal("raw")
	}
}

func TestImportTemplate(t *testing.T) {
	tmp, err := os.CreateTemp("", "cloud-*.img")
	if err != nil {
		t.Fatal(err)
	}
	defer os.Remove(tmp.Name())
	if _, err := tmp.WriteString("qcow2-bytes"); err != nil {
		t.Fatal(err)
	}
	tmp.Close()

	sum, err := FileSHA256(tmp.Name())
	if err != nil || len(sum) != 64 {
		t.Fatalf("hash: %v %s", err, sum)
	}

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.Method == http.MethodPost && r.URL.Path == "/v1/login":
			_ = json.NewEncoder(w).Encode(TokenResponse{Token: "tok-1", Username: "admin", Role: "admin"})
		case r.URL.Path == "/v1/session":
			_ = json.NewEncoder(w).Encode(Session{ID: "u1", Username: "admin", Role: "admin"})
		case r.Method == http.MethodPost && r.URL.Path == "/v1/templates/import":
			if r.URL.Query().Get("name") != "ubuntu-24.04" {
				w.WriteHeader(http.StatusBadRequest)
				_ = json.NewEncoder(w).Encode(ErrorBody{Error: "missing name"})
				return
			}
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(VM{
				ID:    "100",
				Spec:  VMSpec{Name: "ubuntu-24.04", VCPUs: 1, MemoryMiB: 1024},
				State: "stopped",
			})
		case r.Method == http.MethodPost && r.URL.Path == "/v1/templates":
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(VM{
				ID:    "101",
				Spec:  VMSpec{Name: "from-vol", VCPUs: 1, MemoryMiB: 1024},
				State: "stopped",
			})
		case r.Method == http.MethodPost && r.URL.Path == "/v1/vms/110/clone":
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(VM{
				ID:    "200",
				Spec:  VMSpec{Name: "web-1", VCPUs: 2, MemoryMiB: 2048},
				State: "created",
			})
		default:
			w.WriteHeader(http.StatusNotFound)
			_ = json.NewEncoder(w).Encode(ErrorBody{Error: r.URL.Path})
		}
	}))
	defer srv.Close()

	c, err := New(Config{Endpoint: srv.URL, Username: "admin", Password: "admin"})
	if err != nil {
		t.Fatal(err)
	}
	tpl, err := c.ImportTemplate("ubuntu-24.04", "qcow2", tmp.Name(), 1, 1024)
	if err != nil || tpl.ID.String() != "100" {
		t.Fatalf("import: %v %+v", err, tpl)
	}
	cpus, mem := 1, 1024
	fromVol, err := c.CreateTemplate(CreateTemplateRequest{Name: "from-vol", VolumeID: "vol-1", VCPUs: &cpus, MemoryMiB: &mem})
	if err != nil || fromVol.ID.String() != "101" {
		t.Fatalf("create: %v %+v", err, fromVol)
	}
	guest, err := c.CloneVM("110", CloneVMRequest{Name: "web-1"})
	if err != nil || guest.ID.String() != "200" {
		t.Fatalf("clone: %v %+v", err, guest)
	}
}

func TestFindNetworkByName(t *testing.T) {
	if isUUID("lan") || isUUID("vmnet") {
		t.Fatal("names are not UUIDs")
	}
	if !isUUID("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee") {
		t.Fatal("uuid")
	}
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.Method == http.MethodPost && r.URL.Path == "/v1/login":
			_ = json.NewEncoder(w).Encode(TokenResponse{Token: "tok-1", Username: "admin", Role: "admin"})
		case r.URL.Path == "/v1/session":
			_ = json.NewEncoder(w).Encode(Session{ID: "u1", Username: "admin", Role: "admin"})
		case r.Method == http.MethodGet && r.URL.Path == "/v1/networks/lan":
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(ErrorBody{Error: "Invalid URL: Cannot parse `id` with value `lan`: UUID parsing failed"})
		case r.Method == http.MethodGet && r.URL.Path == "/v1/networks":
			_ = json.NewEncoder(w).Encode([]Network{{ID: "net-1", Name: "lan", Mode: "nat"}})
		default:
			w.WriteHeader(http.StatusNotFound)
			_ = json.NewEncoder(w).Encode(ErrorBody{Error: r.URL.Path})
		}
	}))
	defer srv.Close()
	c, err := New(Config{Endpoint: srv.URL, Username: "admin", Password: "admin"})
	if err != nil {
		t.Fatal(err)
	}
	net, err := c.FindNetwork("lan")
	if err != nil || net.ID != "net-1" {
		t.Fatalf("by name: %v %+v", err, net)
	}
}
