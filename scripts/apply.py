#!/usr/bin/env python3
"""Apply WorkloadDeployment manifests to a `wash host` with no Kubernetes.

The runtime-operator turns a Workload into a protojson WorkloadStartRequest
and sends it to runtime.host.<host-id>.workload.start over NATS. This script
does the same conversion for the subset of fields the workshop uses (see
runtime-operator/internal/controller/runtime/workload_controller.go,
reconcilePlacement and materializeLocalResources), so the YAML in steps/ is
the same YAML you would kubectl apply on a cluster.

Differences from the operator, on purpose:
  * hostPath paths may be relative; they resolve against the manifest's
    directory (a cluster needs absolute node paths).
  * environment.config only; configFrom/secretFrom need Kubernetes objects.
  * spec.replicas is ignored: there is one host.

usage:
  apply.py <manifest.yaml>...           start (stops a previous copy first)
  apply.py --delete <manifest.yaml>...  stop
  apply.py --status <manifest.yaml>...  status
"""

import json
import os
import subprocess
import sys

import yaml

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NATS_URL = os.environ.get("NATS_URL", "nats://127.0.0.1:14222")
# Prefer the nats CLI that `make tools` put in .tools/, then whatever is on PATH.
_LOCAL_NATS = os.path.join(ROOT, ".tools", "nats")
NATS = _LOCAL_NATS if os.access(_LOCAL_NATS, os.X_OK) else "nats"


def host_id():
    with open(os.path.join(ROOT, ".run", "host.id")) as f:
        return f.read().strip()


def nats_req(subject, payload, timeout="60s"):
    out = subprocess.run(
        [NATS, "--server", NATS_URL, "req", subject, json.dumps(payload),
         "--timeout", timeout, "--raw"],
        capture_output=True, text=True,
    )
    if out.returncode != 0:
        raise SystemExit(f"nats req {subject} failed: {out.stderr.strip()}")
    return json.loads(out.stdout)


def local_resources(lr, base):
    lr = lr or {}
    out = {}
    if "allowedHosts" in lr:
        out["allowedHosts"] = lr["allowedHosts"]
    if "allowedIpNameLookups" in lr:
        out["allowedIpNameLookups"] = lr["allowedIpNameLookups"]
    if "config" in lr:
        out["config"] = lr["config"]
    env = (lr.get("environment") or {}).get("config")
    if env:
        out["environment"] = env
    if "volumeMounts" in lr:
        out["volumeMounts"] = [
            {"name": m["name"], "mountPath": m["mountPath"],
             "readOnly": bool(m.get("readOnly", False))}
            for m in lr["volumeMounts"]
        ]
    return out


def pull_policy(p):
    return {
        "Always": "IMAGE_PULL_POLICY_ALWAYS",
        "IfNotPresent": "IMAGE_PULL_POLICY_IF_NOT_PRESENT",
        "Never": "IMAGE_PULL_POLICY_NEVER",
    }.get(p or "Always", "IMAGE_PULL_POLICY_ALWAYS")


def to_wire(doc, base):
    meta = doc["metadata"]
    spec = doc["spec"]
    if doc["kind"] == "WorkloadDeployment":
        spec = spec["template"]["spec"]
    elif doc["kind"] != "Workload":
        raise SystemExit(f"unsupported kind {doc['kind']}")
    ns = meta.get("namespace", "default")

    volumes = []
    for v in spec.get("volumes", []):
        if "hostPath" in v:
            path = v["hostPath"]["path"]
            if not os.path.isabs(path):
                path = os.path.normpath(os.path.join(base, path))
            volumes.append({"name": v["name"], "hostPath": {"localPath": path}})
        else:
            volumes.append({"name": v["name"], "emptyDir": {}})

    components = []
    for c in spec.get("components", []):
        wc = {
            "name": c["name"],
            "image": c["image"],
            "imagePullPolicy": pull_policy(c.get("imagePullPolicy")),
            "localResources": local_resources(c.get("localResources"), base),
        }
        for k in ("poolSize", "maxInvocations", "maxConcurrency",
                  "reclaimWindowSeconds", "reclaimMinInstances"):
            if k in c:
                wc[k] = c[k]
        components.append(wc)

    host_interfaces = []
    for hi in spec.get("hostInterfaces", []):
        w = {"namespace": hi["namespace"], "package": hi["package"],
             "interfaces": hi.get("interfaces", [])}
        for k in ("version", "name"):
            if hi.get(k):
                w[k] = hi[k]
        if hi.get("config"):
            w["config"] = hi["config"]
        host_interfaces.append(w)

    return f"{ns}-{meta['name']}", {
        "namespace": ns,
        "name": meta["name"],
        "annotations": meta.get("annotations", {}),
        "witWorld": {"components": components, "hostInterfaces": host_interfaces},
        "volumes": volumes,
    }


def main():
    args = sys.argv[1:]
    mode = "start"
    if args and args[0] in ("--delete", "--status"):
        mode, args = args[0][2:], args[1:]
    hid = host_id()
    for path in args:
        base = os.path.dirname(os.path.abspath(path))
        with open(path) as f:
            docs = [d for d in yaml.safe_load_all(f) if d]
        for doc in docs:
            wid, workload = to_wire(doc, base)
            if mode in ("start", "delete"):
                stop = nats_req(f"runtime.host.{hid}.workload.stop", {"workloadId": wid})
                if mode == "delete":
                    print(f"{wid}: {stop['workloadStatus'].get('workloadState')}")
                    continue
            if mode == "status":
                st = nats_req(f"runtime.host.{hid}.workload.status", {"workloadId": wid})
                s = st["workloadStatus"]
                print(f"{wid}: {s.get('workloadState')} {s.get('message', '')}".rstrip())
                continue
            res = nats_req(f"runtime.host.{hid}.workload.start",
                           {"workloadId": wid, "workload": workload})
            s = res["workloadStatus"]
            print(f"{wid}: {s.get('workloadState')} {s.get('message', '')}".rstrip())
            if s.get("workloadState") != "WORKLOAD_STATE_RUNNING":
                sys.exit(1)


if __name__ == "__main__":
    main()
