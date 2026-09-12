package client

import (
	"bytes"
	"crypto/tls"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"
)

type Client struct {
	endpoint string
	token    string
	http     *http.Client
}

type Config struct {
	Endpoint string
	Token    string
	Username string
	Password string
	Insecure bool
	Timeout  time.Duration
}

type APIError struct {
	Status  int
	Message string
}

func (e *APIError) Error() string {
	if e.Message != "" {
		return fmt.Sprintf("pertisk API %d: %s", e.Status, e.Message)
	}
	return fmt.Sprintf("pertisk API %d", e.Status)
}

func IsNotFound(err error) bool {
	api, ok := err.(*APIError)
	return ok && api.Status == http.StatusNotFound
}

func New(cfg Config) (*Client, error) {
	endpoint := strings.TrimRight(strings.TrimSpace(cfg.Endpoint), "/")
	if endpoint == "" {
		return nil, fmt.Errorf("endpoint is required")
	}
	if _, err := url.Parse(endpoint); err != nil {
		return nil, fmt.Errorf("invalid endpoint: %w", err)
	}
	timeout := cfg.Timeout
	if timeout == 0 {
		timeout = 15 * time.Minute
	}
	transport := http.DefaultTransport.(*http.Transport).Clone()
	if cfg.Insecure {
		if transport.TLSClientConfig == nil {
			transport.TLSClientConfig = &tls.Config{}
		}
		transport.TLSClientConfig.InsecureSkipVerify = true
	}
	c := &Client{
		endpoint: endpoint,
		token:    strings.TrimSpace(cfg.Token),
		http: &http.Client{
			Timeout:   timeout,
			Transport: transport,
		},
	}
	if c.token == "" {
		if cfg.Username == "" || cfg.Password == "" {
			return nil, fmt.Errorf("set token, or username and password")
		}
		tok, err := c.Login(cfg.Username, cfg.Password)
		if err != nil {
			return nil, err
		}
		c.token = tok
	}
	if _, err := c.Session(); err != nil {
		return nil, err
	}
	return c, nil
}

func (c *Client) Login(username, password string) (string, error) {
	var out TokenResponse
	if err := c.do(http.MethodPost, "/v1/login", LoginRequest{Username: username, Password: password}, &out, false); err != nil {
		return "", err
	}
	if out.Token == "" {
		return "", fmt.Errorf("login succeeded but no token was returned")
	}
	return out.Token, nil
}

func (c *Client) Session() (*Session, error) {
	var out Session
	if err := c.do(http.MethodGet, "/v1/session", nil, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) Cluster() (*Cluster, error) {
	var out Cluster
	if err := c.do(http.MethodGet, "/v1/cluster", nil, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) ListVMs() ([]VM, error) {
	var out []VM
	if err := c.do(http.MethodGet, "/v1/vms", nil, &out, true); err != nil {
		return nil, err
	}
	return out, nil
}

func (c *Client) GetVM(id string) (*VM, error) {
	var out VM
	if err := c.do(http.MethodGet, "/v1/vms/"+url.PathEscape(id), nil, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) CreateVM(req CreateVMRequest) (*VM, error) {
	var out VM
	if err := c.do(http.MethodPost, "/v1/vms", req, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) UpdateVM(id string, req UpdateVMRequest) (*VM, error) {
	var out VM
	if err := c.do(http.MethodPatch, "/v1/vms/"+url.PathEscape(id), req, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) DeleteVM(id string) error {
	return c.do(http.MethodDelete, "/v1/vms/"+url.PathEscape(id), nil, nil, true)
}

func (c *Client) StartVM(id string) (*VM, error) {
	var out VM
	if err := c.do(http.MethodPost, "/v1/vms/"+url.PathEscape(id)+"/start", map[string]any{}, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) StopVM(id string) (*VM, error) {
	var out VM
	if err := c.do(http.MethodPost, "/v1/vms/"+url.PathEscape(id)+"/stop", map[string]any{}, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) CloneVM(id string, req CloneVMRequest) (*VM, error) {
	var out VM
	if err := c.do(http.MethodPost, "/v1/vms/"+url.PathEscape(id)+"/clone", req, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) AttachDisk(vmID, volumeID string) (*VM, error) {
	var out VM
	if err := c.do(http.MethodPost, "/v1/vms/"+url.PathEscape(vmID)+"/disks", AttachDiskRequest{VolumeID: volumeID}, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) AttachISO(vmID, iso string) (*VM, error) {
	var out VM
	if err := c.do(http.MethodPost, "/v1/vms/"+url.PathEscape(vmID)+"/cdrom", AttachISORequest{ISO: iso}, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) AttachNic(vmID, networkID, ip string) (*VM, error) {
	var out VM
	body := AttachNicRequest{NetworkID: networkID, IP: ip}
	if err := c.do(http.MethodPost, "/v1/vms/"+url.PathEscape(vmID)+"/nics", body, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) CreateCloudInitISO(req CloudInitISORequest) (*ISO, error) {
	var out ISO
	if err := c.do(http.MethodPost, "/v1/isos/cloud-init", req, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) ListTemplates() ([]VM, error) {
	var out []VM
	if err := c.do(http.MethodGet, "/v1/templates", nil, &out, true); err != nil {
		return nil, err
	}
	return out, nil
}

func (c *Client) CreateTemplate(req CreateTemplateRequest) (*VM, error) {
	var out VM
	if err := c.do(http.MethodPost, "/v1/templates", req, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) ConvertToTemplate(id string) (*VM, error) {
	var out VM
	if err := c.do(http.MethodPost, "/v1/vms/"+url.PathEscape(id)+"/template", map[string]any{}, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) ImportTemplate(name, format, imagePath string, vcpus, memory int) (*VM, error) {
	path := ExpandPath(imagePath)
	f, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("open image: %w", err)
	}
	defer f.Close()
	stat, err := f.Stat()
	if err != nil {
		return nil, err
	}
	q := url.Values{}
	q.Set("name", name)
	if format != "" {
		q.Set("format", format)
	}
	if vcpus > 0 {
		q.Set("vcpus", strconv.Itoa(vcpus))
	}
	if memory > 0 {
		q.Set("memory_mib", strconv.Itoa(memory))
	}
	req, err := http.NewRequest(http.MethodPost, c.endpoint+"/v1/templates/import?"+q.Encode(), f)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/octet-stream")
	if c.token != "" {
		req.Header.Set("Authorization", "Bearer "+c.token)
	}
	req.ContentLength = stat.Size()
	httpClient := *c.http
	if httpClient.Timeout < 60*time.Minute {
		httpClient.Timeout = 60 * time.Minute
	}
	res, err := httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer res.Body.Close()
	var out VM
	if err := decodeResponse(res, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) FindVM(idOrName string) (*VM, error) {
	if vm, err := c.GetVM(idOrName); err == nil {
		return vm, nil
	} else if !IsNotFound(err) {
		return nil, err
	}
	vms, err := c.ListVMs()
	if err != nil {
		return nil, err
	}
	var match *VM
	for i := range vms {
		if vms[i].Spec.Name == idOrName {
			if match != nil {
				return nil, fmt.Errorf("multiple guests named %q", idOrName)
			}
			copy := vms[i]
			match = &copy
		}
	}
	if match == nil {
		return nil, &APIError{Status: http.StatusNotFound, Message: fmt.Sprintf("guest %q not found", idOrName)}
	}
	return match, nil
}

func (c *Client) FindTemplate(idOrName string) (*VM, error) {
	templates, err := c.ListTemplates()
	if err != nil {
		return nil, err
	}
	var match *VM
	for i := range templates {
		if templates[i].ID.String() == idOrName || templates[i].Spec.Name == idOrName {
			if match != nil && templates[i].ID.String() != idOrName {
				return nil, fmt.Errorf("multiple templates named %q", idOrName)
			}
			copy := templates[i]
			match = &copy
			if templates[i].ID.String() == idOrName {
				return match, nil
			}
		}
	}
	if match == nil {
		return nil, &APIError{Status: http.StatusNotFound, Message: fmt.Sprintf("template %q not found", idOrName)}
	}
	return match, nil
}

func (c *Client) ListNetworks() ([]Network, error) {
	var out []Network
	if err := c.do(http.MethodGet, "/v1/networks", nil, &out, true); err != nil {
		return nil, err
	}
	return out, nil
}

func (c *Client) GetNetwork(id string) (*Network, error) {
	var out Network
	if err := c.do(http.MethodGet, "/v1/networks/"+url.PathEscape(id), nil, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) CreateNetwork(req CreateNetworkRequest) (*Network, error) {
	var out Network
	if err := c.do(http.MethodPost, "/v1/networks", req, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) DeleteNetwork(id string) error {
	return c.do(http.MethodDelete, "/v1/networks/"+url.PathEscape(id), nil, nil, true)
}

func (c *Client) FindNetwork(idOrName string) (*Network, error) {
	if net, err := c.GetNetwork(idOrName); err == nil {
		return net, nil
	} else if !IsNotFound(err) {
		return nil, err
	}
	nets, err := c.ListNetworks()
	if err != nil {
		return nil, err
	}
	var match *Network
	for i := range nets {
		if nets[i].Name == idOrName {
			if match != nil {
				return nil, fmt.Errorf("multiple networks named %q", idOrName)
			}
			copy := nets[i]
			match = &copy
		}
	}
	if match == nil {
		return nil, &APIError{Status: http.StatusNotFound, Message: fmt.Sprintf("network %q not found", idOrName)}
	}
	return match, nil
}

func (c *Client) ListVolumes() ([]Volume, error) {
	var out []Volume
	if err := c.do(http.MethodGet, "/v1/volumes", nil, &out, true); err != nil {
		return nil, err
	}
	return out, nil
}

func (c *Client) GetVolume(id string) (*Volume, error) {
	var out Volume
	if err := c.do(http.MethodGet, "/v1/volumes/"+url.PathEscape(id), nil, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) CreateVolume(req CreateVolumeRequest) (*Volume, error) {
	var out Volume
	if err := c.do(http.MethodPost, "/v1/volumes", req, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) ResizeVolume(id string, sizeBytes uint64) (*Volume, error) {
	var out Volume
	if err := c.do(http.MethodPost, "/v1/volumes/"+url.PathEscape(id)+"/resize", ResizeVolumeRequest{SizeBytes: sizeBytes}, &out, true); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *Client) DeleteVolume(id string) error {
	return c.do(http.MethodDelete, "/v1/volumes/"+url.PathEscape(id), nil, nil, true)
}

func (c *Client) FindVolume(idOrName string) (*Volume, error) {
	if vol, err := c.GetVolume(idOrName); err == nil {
		return vol, nil
	} else if !IsNotFound(err) {
		return nil, err
	}
	vols, err := c.ListVolumes()
	if err != nil {
		return nil, err
	}
	var match *Volume
	for i := range vols {
		if vols[i].Name == idOrName {
			if match != nil {
				return nil, fmt.Errorf("multiple volumes named %q", idOrName)
			}
			copy := vols[i]
			match = &copy
		}
	}
	if match == nil {
		return nil, &APIError{Status: http.StatusNotFound, Message: fmt.Sprintf("volume %q not found", idOrName)}
	}
	return match, nil
}

func NumericID(id string) any {
	id = strings.TrimSpace(id)
	if id == "" {
		return nil
	}
	if n, err := strconv.ParseUint(id, 10, 64); err == nil {
		return n
	}
	return id
}

func (c *Client) do(method, path string, in, out any, auth bool) error {
	var body io.Reader
	if in != nil {
		raw, err := json.Marshal(in)
		if err != nil {
			return err
		}
		body = bytes.NewReader(raw)
	}
	req, err := http.NewRequest(method, c.endpoint+path, body)
	if err != nil {
		return err
	}
	if in != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if auth && c.token != "" {
		req.Header.Set("Authorization", "Bearer "+c.token)
	}
	res, err := c.http.Do(req)
	if err != nil {
		return err
	}
	defer res.Body.Close()
	return decodeResponse(res, out)
}

func decodeResponse(res *http.Response, out any) error {
	raw, err := io.ReadAll(res.Body)
	if err != nil {
		return err
	}
	if res.StatusCode == http.StatusNoContent {
		return nil
	}
	if res.StatusCode >= 400 {
		msg := strings.TrimSpace(string(raw))
		var parsed ErrorBody
		if json.Unmarshal(raw, &parsed) == nil && parsed.Error != "" {
			msg = parsed.Error
		}
		return &APIError{Status: res.StatusCode, Message: msg}
	}
	if out == nil || len(raw) == 0 {
		return nil
	}
	if err := json.Unmarshal(raw, out); err != nil {
		return fmt.Errorf("decode %s %s: %w", res.Request.Method, res.Request.URL.Path, err)
	}
	return nil
}
