# Extra: the same agent as an ordinary container

Not a step in the project. This runs from the presentation, to make the
contrast the workshop rests on.

A Job runs a few lines of shell that do what an agent harness does when its
model is tricked: fetch a page, obey the instruction hidden in it, and send
what it can reach to the collector. On the default Kubernetes setup it reaches
a Secret in an environment variable and the pod's service account token, and
sends both in full. Nothing here is unusual: a Secret in an environment
variable, the default service account, the default network. The point is that
those defaults start open, so a tricked agent in a plain container leaks what
the pod can see, while the same trick against a sandboxed tool reaches only
what that tool was granted.

**Run** (presenter):

```sh
kubectl apply -f extras/container/gullible-agent.yaml
kubectl wait --for=condition=complete job/gullible-agent --timeout=120s
kubectl logs job/gullible-agent
curl -s -H 'Host: sites.localhost' http://127.0.0.1:18180/count    # the leak arrived
```

The defaults can each be closed (a projected service account token with a short
lifetime, no token mounted at all, a restrictive NetworkPolicy, the Secret out
of the environment). This Job shows what you get when they are not.
