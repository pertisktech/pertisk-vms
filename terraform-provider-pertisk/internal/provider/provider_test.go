package provider

import (
	"testing"

	"github.com/hashicorp/terraform-plugin-framework/providerserver"
	"github.com/hashicorp/terraform-plugin-go/tfprotov6"
)

func testAccProtoV6ProviderFactories() map[string]func() (tfprotov6.ProviderServer, error) {
	return map[string]func() (tfprotov6.ProviderServer, error){
		"pertisk": providerserver.NewProtocol6WithError(New("test")()),
	}
}

func TestProviderFactories(t *testing.T) {
	factories := testAccProtoV6ProviderFactories()
	if _, ok := factories["pertisk"]; !ok {
		t.Fatal("missing pertisk factory")
	}
}
