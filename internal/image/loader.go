package image

import (
	"fmt"
	"os"

	"github.com/google/go-containerregistry/pkg/authn"
	"github.com/google/go-containerregistry/pkg/name"
	v1 "github.com/google/go-containerregistry/pkg/v1"
	"github.com/google/go-containerregistry/pkg/v1/daemon"
	"github.com/google/go-containerregistry/pkg/v1/remote"
	"github.com/google/go-containerregistry/pkg/v1/tarball"
)

// Load detects the source type and returns a v1.Image.
// If source is an existing file, loads as tarball.
// Otherwise tries daemon, then remote registry.
func Load(source string) (v1.Image, error) {
	if info, err := os.Stat(source); err == nil && !info.IsDir() {
		return loadFromTarball(source)
	}

	ref, err := name.ParseReference(source)
	if err != nil {
		return nil, fmt.Errorf("not a file and not a valid image reference: %w", err)
	}

	img, err := loadFromDaemon(ref)
	if err == nil {
		return img, nil
	}

	return loadFromRemote(ref)
}

func loadFromTarball(path string) (v1.Image, error) {
	img, err := tarball.ImageFromPath(path, nil)
	if err != nil {
		return nil, fmt.Errorf("loading tarball: %w", err)
	}
	return img, nil
}

func loadFromDaemon(ref name.Reference) (v1.Image, error) {
	img, err := daemon.Image(ref)
	if err != nil {
		return nil, fmt.Errorf("loading from daemon: %w", err)
	}
	return img, nil
}

func loadFromRemote(ref name.Reference) (v1.Image, error) {
	img, err := remote.Image(ref, remote.WithAuthFromKeychain(authn.DefaultKeychain))
	if err != nil {
		return nil, fmt.Errorf("pulling from registry: %w", err)
	}
	return img, nil
}
