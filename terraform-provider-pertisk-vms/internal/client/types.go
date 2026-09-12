package client

import (
	"bytes"
	"encoding/json"
	"strconv"
	"strings"
)

// FlexID unmarshals JSON string or number identifiers (numeric VM IDs serialize as numbers).
type FlexID string

func (id *FlexID) UnmarshalJSON(b []byte) error {
	b = bytes.TrimSpace(b)
	if bytes.Equal(b, []byte("null")) {
		*id = ""
		return nil
	}
	if len(b) > 0 && b[0] == '"' {
		var s string
		if err := json.Unmarshal(b, &s); err != nil {
			return err
		}
		*id = FlexID(s)
		return nil
	}
	*id = FlexID(strings.TrimSpace(string(b)))
	return nil
}

func (id FlexID) MarshalJSON() ([]byte, error) {
	s := string(id)
	if s == "" {
		return []byte("null"), nil
	}
	if n, err := strconv.ParseUint(s, 10, 64); err == nil {
		return []byte(strconv.FormatUint(n, 10)), nil
	}
	return json.Marshal(s)
}

func (id FlexID) String() string { return string(id) }

type ErrorBody struct {
	Error string `json:"error"`
}

type TokenResponse struct {
	Token    string `json:"token"`
	Username string `json:"username"`
	Role     string `json:"role"`
}

type LoginRequest struct {
	Username string `json:"username"`
	Password string `json:"password"`
}

type Session struct {
	ID       string `json:"id"`
	Username string `json:"username"`
	Role     string `json:"role"`
}

type Disk struct {
	Path     string `json:"path,omitempty"`
	Readonly bool   `json:"readonly,omitempty"`
	Cdrom    bool   `json:"cdrom,omitempty"`
	VolumeID string `json:"volume_id,omitempty"`
	ISOName  string `json:"iso_name,omitempty"`
}

type Nic struct {
	NetworkID string `json:"network_id,omitempty"`
	Tap       string `json:"tap,omitempty"`
	MAC       string `json:"mac,omitempty"`
	IP        string `json:"ip,omitempty"`
	IPv6      string `json:"ipv6,omitempty"`
}

type VMSpec struct {
	Name           string `json:"name"`
	VCPUs          int    `json:"vcpus"`
	MemoryMiB      int    `json:"memory_mib"`
	Kernel         string `json:"kernel,omitempty"`
	Cmdline        string `json:"cmdline,omitempty"`
	Initramfs      string `json:"initramfs,omitempty"`
	Firmware       string `json:"firmware,omitempty"`
	Disks          []Disk `json:"disks,omitempty"`
	Nets           []Nic  `json:"nets,omitempty"`
	ConsoleType    string `json:"console_type,omitempty"`
	HA             bool   `json:"ha"`
	Autostart      bool   `json:"autostart,omitempty"`
	AutostartDelay uint64 `json:"autostart_delay,omitempty"`
	AutostartOrder uint32 `json:"autostart_order,omitempty"`
}

type VM struct {
	ID        FlexID `json:"id"`
	Spec      VMSpec `json:"spec"`
	State     string `json:"state"`
	NodeID    string `json:"node_id,omitempty"`
	Template  bool   `json:"template,omitempty"`
	LastError string `json:"last_error,omitempty"`
}

type CreateVMRequest struct {
	ID             any    `json:"id,omitempty"`
	Name           string `json:"name"`
	VCPUs          int    `json:"vcpus"`
	MemoryMiB      int    `json:"memory_mib"`
	ConsoleType    string `json:"console_type,omitempty"`
	HA             bool   `json:"ha"`
	Autostart      bool   `json:"autostart,omitempty"`
	AutostartDelay uint64 `json:"autostart_delay,omitempty"`
	AutostartOrder uint32 `json:"autostart_order,omitempty"`
	Kernel         string `json:"kernel,omitempty"`
	Initramfs      string `json:"initramfs,omitempty"`
	Firmware       string `json:"firmware,omitempty"`
	Cmdline        string `json:"cmdline,omitempty"`
}

type UpdateVMRequest struct {
	Name           *string `json:"name,omitempty"`
	VCPUs          *int    `json:"vcpus,omitempty"`
	MemoryMiB      *int    `json:"memory_mib,omitempty"`
	HA             *bool   `json:"ha,omitempty"`
	Autostart      *bool   `json:"autostart,omitempty"`
	AutostartDelay *uint64 `json:"autostart_delay,omitempty"`
	AutostartOrder *uint32 `json:"autostart_order,omitempty"`
}

type CloudInit struct {
	Hostname string   `json:"hostname,omitempty"`
	User     string   `json:"user,omitempty"`
	Password string   `json:"password,omitempty"`
	SSHKeys  []string `json:"ssh_authorized_keys,omitempty"`
	Userdata string   `json:"userdata,omitempty"`
}

type CreateTemplateRequest struct {
	ID          any    `json:"id,omitempty"`
	Name        string `json:"name"`
	VolumeID    string `json:"volume_id"`
	VCPUs       *int   `json:"vcpus,omitempty"`
	MemoryMiB   *int   `json:"memory_mib,omitempty"`
	ConsoleType string `json:"console_type,omitempty"`
}

type CloneVMRequest struct {
	ID            any        `json:"id,omitempty"`
	Name          string     `json:"name"`
	Linked        bool       `json:"linked,omitempty"`
	VCPUs         *int       `json:"vcpus,omitempty"`
	MemoryMiB     *int       `json:"memory_mib,omitempty"`
	HA            *bool      `json:"ha,omitempty"`
	Autostart     *bool      `json:"autostart,omitempty"`
	NetworkID     string     `json:"network_id,omitempty"`
	IP            string     `json:"ip,omitempty"`
	CloudInit     *CloudInit `json:"cloud_init,omitempty"`
	DiskSizeBytes *uint64    `json:"disk_size_bytes,omitempty"`
	Start         bool       `json:"start,omitempty"`
}

type AttachDiskRequest struct {
	VolumeID string `json:"volume_id"`
}

type AttachISORequest struct {
	ISO string `json:"iso"`
}

type AttachNicRequest struct {
	NetworkID string `json:"network_id"`
	IP        string `json:"ip,omitempty"`
}

type CloudInitISORequest struct {
	Name     string   `json:"name"`
	Hostname string   `json:"hostname,omitempty"`
	User     string   `json:"user,omitempty"`
	Password string   `json:"password,omitempty"`
	SSHKeys  []string `json:"ssh_authorized_keys,omitempty"`
	Userdata string   `json:"userdata,omitempty"`
}

type ISO struct {
	Name      string `json:"name"`
	Path      string `json:"path,omitempty"`
	SizeBytes uint64 `json:"size_bytes"`
}

type Network struct {
	ID      string `json:"id"`
	Name    string `json:"name"`
	Bridge  string `json:"bridge"`
	CIDR    string `json:"cidr"`
	Gateway string `json:"gateway,omitempty"`
	DHCP    bool   `json:"dhcp"`
	Isolate bool   `json:"isolate"`
	Mode    string `json:"mode"`
}

type CreateNetworkRequest struct {
	Name    string `json:"name"`
	CIDR    string `json:"cidr,omitempty"`
	Gateway string `json:"gateway,omitempty"`
	Bridge  string `json:"bridge,omitempty"`
	DHCP    *bool  `json:"dhcp,omitempty"`
	Isolate *bool  `json:"isolate,omitempty"`
	Mode    string `json:"mode,omitempty"`
}

type Volume struct {
	ID           string   `json:"id"`
	Name         string   `json:"name"`
	Format       string   `json:"format"`
	SizeBytes    uint64   `json:"size_bytes"`
	Path         string   `json:"path,omitempty"`
	Replicas     []string `json:"replicas,omitempty"`
	ReplicaCount int      `json:"replica_count,omitempty"`
	Backend      string   `json:"backend,omitempty"`
}

type CreateVolumeRequest struct {
	Name      string `json:"name"`
	SizeBytes uint64 `json:"size_bytes"`
	Format    string `json:"format,omitempty"`
	Replicas  *int   `json:"replicas,omitempty"`
}

type ResizeVolumeRequest struct {
	SizeBytes uint64 `json:"size_bytes"`
}

type ClusterMember struct {
	ID            string   `json:"id"`
	Name          string   `json:"name"`
	PeerURL       string   `json:"peer_url"`
	Online        bool     `json:"online"`
	CPUs          uint32   `json:"cpus"`
	MemoryMiB     uint32   `json:"memory_mib"`
	UsedVCPUs     uint32   `json:"used_vcpus"`
	UsedMemoryMiB uint32   `json:"used_memory_mib"`
	IPv4          []string `json:"ipv4,omitempty"`
	IPv6          []string `json:"ipv6,omitempty"`
}

type Cluster struct {
	Name       string          `json:"name"`
	Generation uint64          `json:"generation"`
	SelfID     string          `json:"self_id"`
	LeaderID   string          `json:"leader_id,omitempty"`
	Quorum     bool            `json:"quorum"`
	Fenced     bool            `json:"fenced"`
	Members    []ClusterMember `json:"members"`
}
