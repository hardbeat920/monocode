// Userspace Tailscale node for the iPad companion (libtailscale-style c-archive).
//
// The iPad reaches the Mac's tailnet IP without the Tailscale iOS app and
// without a Network Extension / packet-tunnel entitlement. Google SSO is
// the same login.tailscale.com flow the Mac embed uses: Start() then poll
// status JSON for AuthURL / Running.
package main

/*
#include <stdlib.h>
*/
import "C"

import (
	"context"
	"encoding/json"
	"io"
	"os"
	"sync"
	"syscall"
	"time"
	"unsafe"

	"tailscale.com/hostinfo"
	"tailscale.com/tsnet"
)

func main() {}

var servers struct {
	mu   sync.Mutex
	next C.int
	m    map[C.int]*server
}

type server struct {
	s       *tsnet.Server
	lastErr string
	started bool
}

type conn struct {
	c netConn
	r *os.File
}

// netConn is the subset of net.Conn we copy. Avoids importing net in the
// type identity of the map (the real type is net.Conn from tsnet.Dial).
type netConn interface {
	io.ReadWriteCloser
}

var conns struct {
	mu sync.Mutex
	m  map[C.int]*conn
}

func getServer(sd C.int) *server {
	servers.mu.Lock()
	defer servers.mu.Unlock()
	return servers.m[sd]
}

func (s *server) recErr(err error) C.int {
	if err == nil {
		s.lastErr = ""
		return 0
	}
	s.lastErr = err.Error()
	return -1
}

//export monocode_tsnet_new
func monocode_tsnet_new() C.int {
	servers.mu.Lock()
	defer servers.mu.Unlock()
	if servers.m == nil {
		servers.m = map[C.int]*server{}
		hostinfo.SetApp("monocode")
	}
	if servers.next == 0 {
		servers.next = 42<<16 + 1
	}
	sd := servers.next
	servers.next++
	servers.m[sd] = &server{s: &tsnet.Server{}}
	return sd
}

//export monocode_tsnet_start
func monocode_tsnet_start(sd C.int) C.int {
	s := getServer(sd)
	if s == nil {
		return -1
	}
	err := s.s.Start()
	if err == nil {
		s.started = true
	}
	return s.recErr(err)
}

//export monocode_tsnet_close
func monocode_tsnet_close(sd C.int) C.int {
	servers.mu.Lock()
	s := servers.m[sd]
	if s != nil {
		delete(servers.m, sd)
	}
	servers.mu.Unlock()
	if s == nil {
		return -1
	}
	if !s.started {
		return 0
	}
	if err := s.s.Close(); err != nil {
		return -1
	}
	return 0
}

//export monocode_tsnet_set_dir
func monocode_tsnet_set_dir(sd C.int, dir *C.char) C.int {
	s := getServer(sd)
	if s == nil {
		return -1
	}
	s.s.Dir = C.GoString(dir)
	return 0
}

//export monocode_tsnet_set_hostname
func monocode_tsnet_set_hostname(sd C.int, hostname *C.char) C.int {
	s := getServer(sd)
	if s == nil {
		return -1
	}
	s.s.Hostname = C.GoString(hostname)
	return 0
}

//export monocode_tsnet_set_authkey
func monocode_tsnet_set_authkey(sd C.int, key *C.char) C.int {
	s := getServer(sd)
	if s == nil {
		return -1
	}
	s.s.AuthKey = C.GoString(key)
	return 0
}

//export monocode_tsnet_errmsg
func monocode_tsnet_errmsg(sd C.int, buf *C.char, buflen C.size_t) C.int {
	if buf == nil || buflen == 0 {
		return -1
	}
	out := unsafe.Slice((*byte)(unsafe.Pointer(buf)), buflen)
	s := getServer(sd)
	msg := ""
	if s != nil {
		msg = s.lastErr
	} else {
		msg = "invalid tailnet handle"
	}
	n := copy(out, msg)
	if n >= len(out) {
		out[len(out)-1] = 0
		return -1
	}
	out[n] = 0
	return 0
}

//export monocode_tsnet_status_json
func monocode_tsnet_status_json(sd C.int, jsonOut **C.char) C.int {
	if jsonOut == nil {
		return -1
	}
	*jsonOut = nil
	s := getServer(sd)
	if s == nil {
		return -1
	}
	lc, err := s.s.LocalClient()
	if err != nil {
		return s.recErr(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	st, err := lc.Status(ctx)
	if err != nil {
		return s.recErr(err)
	}
	b, err := json.Marshal(st)
	if err != nil {
		return s.recErr(err)
	}
	*jsonOut = C.CString(string(b))
	return 0
}

func newConn(netC io.ReadWriteCloser, connOut *C.int) error {
	fds, err := syscall.Socketpair(syscall.AF_UNIX, syscall.SOCK_STREAM, 0)
	if err != nil {
		return err
	}
	r := os.NewFile(uintptr(fds[1]), "socketpair-r")
	c := &conn{c: netC, r: r}
	fdC := C.int(fds[0])

	conns.mu.Lock()
	if conns.m == nil {
		conns.m = map[C.int]*conn{}
	}
	conns.m[fdC] = c
	conns.mu.Unlock()

	cleanup := func() {
		conns.mu.Lock()
		live, ok := conns.m[fdC]
		if ok && live.c == netC {
			delete(conns.m, fdC)
		} else {
			ok = false
		}
		conns.mu.Unlock()
		if !ok {
			return
		}
		r.Close()
		netC.Close()
	}
	go func() {
		defer cleanup()
		var b [1 << 16]byte
		io.CopyBuffer(r, netC, b[:])
		_ = syscall.Shutdown(int(r.Fd()), syscall.SHUT_WR)
	}()
	go func() {
		defer cleanup()
		var b [1 << 16]byte
		io.CopyBuffer(netC, r, b[:])
		_ = syscall.Shutdown(int(r.Fd()), syscall.SHUT_RD)
	}()
	*connOut = fdC
	return nil
}

//export monocode_tsnet_dial
func monocode_tsnet_dial(sd C.int, network, addr *C.char, connOut *C.int) C.int {
	s := getServer(sd)
	if s == nil {
		return -1
	}
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()
	netC, err := s.s.Dial(ctx, C.GoString(network), C.GoString(addr))
	if err != nil {
		return s.recErr(err)
	}
	s.started = true
	if err := newConn(netC, connOut); err != nil {
		netC.Close()
		return s.recErr(err)
	}
	return 0
}
