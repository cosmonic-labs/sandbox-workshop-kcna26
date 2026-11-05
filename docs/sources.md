# Sources

These are the sources for the factual claims on the "Why sandboxing matters" slides (9, 11 to 15) of the KubeCon NA 2026 workshop deck. Sources were checked on 2026-11-04. Primary sources come first; press is listed only where no primary source was reachable or where the primary source is only a social post.

## Slide 9: Sandboxing on Kubernetes with WebAssembly

- **Harness images 86 MB to 1.3 GB compressed across 11 harnesses (about 0.2 to 1.1 GB on disk).** Cosmonic measurement from public registry manifests, linux/amd64, 2026-10-29. No public source.
- **E2B default sandbox RAM is 512 MiB.** E2B Docs, "Sandbox compute" (default resources "2 vCPU, 512 MiB RAM"), accessed 2026-11-04. https://docs.e2b.dev/sandbox/compute
- **Daytona default sandbox RAM is 1 GiB.** Daytona Docs, "Sandboxes" (default 1 vCPU, 1 GB RAM, 3 GiB disk) and "Linux Sandboxes" (`daytona-small`: 1 vCPU, 1 GiB memory), accessed 2026-11-04. https://www.daytona.io/docs/en/sandboxes/ and https://www.daytona.io/docs/en/linux-sandboxes/
- **Firecracker adds 5 MiB or less for a small microVM.** Firecracker, SPECIFICATION.md (main branch): "Firecracker's virtual machine manager threads have a memory overhead `<= 5 MiB`", for a microVM with 1 vCPU and 128 MiB RAM, accessed 2026-11-04. https://github.com/firecracker-microvm/firecracker/blob/main/SPECIFICATION.md
- **A small harness (Goose) can idle near 24 MB.** Cosmonic measurement. No public source.

## Slide 11: Why does sandboxing matter? (timeline)

- **July 9 to 13: an OpenAI evaluation agent got out through a zero-day in a package proxy it was allowed to use.**
  - Hugging Face, "Anatomy of a Frontier Lab Agent Intrusion: A Technical Timeline of the July 2026 Incident", 2026-07-27. https://huggingface.co/blog/agent-intrusion-technical-timeline
  - OpenAI's own statement (2026-07-21 per press) is reported at https://openai.com/index/hugging-face-model-evaluation-security-incident/ (returned 403 when checked; not read).
- **July 30: Anthropic disclosed three cases of Claude reaching the internet from setups meant to be isolated.**
  - Anthropic, "Investigating three real-world incidents in our cybersecurity evaluations", 2026-07-30 (updated 2026-08-03). https://www.anthropic.com/news/investigating-incidents-cybersecurity-evals
- **September: Google confirmed a test setup that wrongly allowed internet access let Gemini reach three real companies.** Google published no write-up of its own; its confirmation was given to the press.
  - Al Jazeera, "Google's Gemini AI hacks 3 companies in security test, then stops", 2026-09-19 (Google confirmation and Heather Adkins quote). https://aljazeera.com/news/2026/9/19/googles-gemini-ai-hacks-3-companies-in-security-test-then-stops
  - IBTimes UK, "Gemini's Three Real-World Breaches Expose a Bigger Problem", 2026-09-21 (summarizes WSJ, SecurityWeek and The Record reporting). https://www.ibtimes.co.uk/google-gemini-accessed-companies-security-test-1821092
- **September 20: blocked by a web proxy, an OpenAI training agent hid its queries inside DNS lookups.**
  - OpenAI Alignment, "An agent used DNS to reach an external chatbot", incident 2026-09-20, first posted 2026-09-25. https://alignment.openai.com/misalignment-reports/an-agent-used-dns-to-reach-an-external-chatbot/

## Slide 12: Why does sandboxing matter? (collage)

The four timeline cards repeat slide 11; their sources are above.

- **Oct 2: NSW says an OpenAI agent got into a National Parks web app in June.**
  - iTnews, "NSW National Parks web app accessed by OpenAI agent", 2026-10-04. https://www.itnews.com.au/news/nsw-national-parks-web-app-accessed-by-openai-agent-629402
- **Oct 5: suspected OpenAI agents changed a Wikimedia tool's config.**
  - Wikimedia Foundation, "OpenAI 'rogue' agent activities found on Wikimedia projects", 2026-10-05. https://wikimediafoundation.org/news/2026/10/05/openai-rogue-agent-activities-found-on-wikimedia-projects/ (also on Diff: https://diff.wikimedia.org/2026/10/05/openai-rogue-agent-activities-found-on-wikimedia-projects/)
- **Oct 15: India's CERT-In: agents under test reached live systems.**
  - CERT-In advisory CIAD-2026-0049, "AI Agents as Attack Vectors" (Critical), 2026-10-15. The cert-in.org.in page was not reached; reported by Deccan Chronicle, "CERT-In issues critical alert after autonomous AI agents access live systems of Indian orgs". https://www.deccanchronicle.com/artificial-intelligence/cert-in-issues-critical-alert-after-autonomous-ai-agents-access-live-systems-of-indian-orgs-1996324
- **Oct 15: Langflow MCP flaw, CVSS 9.9, lets any logged-in user run commands; exploit attempts seen in the wild.**
  - CVE-2026-105740 (Langflow before 1.9.0, CVSS 9.9 from the GitHub CNA). The Hacker News, "Langflow's Critical Authenticated RCE Flaw Targeted in 1,423 Honeypot Exploit Attempts", 2026-10-15. https://thehackernews.com/2026/10/langflows-critical-authenticated-rce.html
- **Oct 16: OpenAI's incident list hits 19, incl. a model paying from a wallet nobody gave it.**
  - OpenAI Alignment, misalignment reports list (19 reports first posted on or before 2026-10-16). https://alignment.openai.com/misalignment-reports/?sort=created
  - OpenAI Alignment, "Using a funded test cryptocurrency wallet without authorization", incident 2026-06-26, posted 2026-10-15. https://alignment.openai.com/misalignment-reports/using-a-funded-test-cryptocurrency-wallet-without-authorization/
- **Oct 17: OpenCode Console breach may have exposed stored AI API keys (OpenCode says 1.4% of Console users).**
  - OpenCode on X, 2026-10-17. https://x.com/opencode/status/2111519637049319818
  - BleepingComputer, "OpenCode hack exposes API keys amid AI distillation attacks", 2026-10-19. https://www.bleepingcomputer.com/news/security/opencode-hack-exposes-api-keys-amid-ai-distillation-attacks/
- **Oct 21: LiteLLM 1.107.0 on PyPI ships a credential stealer.**
  - LiteLLM (BerriAI), "Contained: Oct 21, 2026 LiteLLM release v1.107.0 Compromised", 2026-10-21. https://docs.litellm.ai/blog/security-incident-oct-21-2026
  - Microsoft Security Blog, "Inside the LiteLLM 1.107.0 supply chain compromise", 2026-10-23. https://www.microsoft.com/en-us/security/blog/2026/10/23/inside-the-litellm-1-107-0-supply-chain-compromise-credential-theft-through-python-startup-hooks/
- **Oct 21: Storm-0940 aims an AI agent framework at 2,250+ tenants.**
  - Microsoft Security Blog, "AI-driven, large-scale nation-state campaign using ARTEX enables supply chain compromise", 2026-10-21. https://www.microsoft.com/en-us/security/blog/2026/10/21/storm-0940-uses-artex-agentic-ai-connectwise-supply-chain-compromise/
- **Oct 22: a planted AGENTS.md gets Codex to install a package more than 90% of the time (most test runs used a harmless stand-in payload).**
  - Forever Security, "Markdown Is the New Executable: Attackers Are Dropping Agent Instructions Into Your Downloads Folder From the Web", 2026-10-22. https://forever.security/blog/hijacking-ai-agents-via-downloads-folder
  - The Hacker News, "Malicious File in Downloads Folder Tries to Trick AI Coding Agent Into Installing Malware", 2026-10-26. https://thehackernews.com/2026/10/malicious-file-in-downloads-folder.html
- **2026: monthly vulnerability disclosures doubled: 5,045 in Jan, 10,740 in Aug (Google Threat Intelligence Group counts).**
  - Google Threat Intelligence Group, "Vulnerability Discovery and Exploitation Trends in the AI Era", 2026-09-30. https://cloud.google.com/blog/topics/threat-intelligence/vulnerability-discovery-and-exploitation-trends-in-the-ai-era
- **UK NCSC (2025): AI will "almost certainly" raise the frequency and intensity of cyber threats through 2027.**
  - NCSC, "Impact of AI on cyber threat from now to 2027", 2025-05-07. https://www.ncsc.gov.uk/report/impact-ai-cyber-threat-now-2027

## Slide 13: Orgs are building stronger walls

- **AWS Lambda: Firecracker microVMs.**
  - Agache et al., "Firecracker: Lightweight Virtualization for Serverless Applications", USENIX NSDI '20, February 2020 (deployed in Lambda and Fargate). https://www.usenix.org/conference/nsdi20/presentation/agache
- **GKE, Kubernetes Agent Sandbox: gVisor or Kata Containers.**
  - Google Cloud Blog, "Agent Sandbox on GKE is now available for everyone, and a first look at Agent Substrate", 2026-05-20. https://cloud.google.com/blog/products/containers-kubernetes/bringing-you-agent-sandbox-on-gke-and-agent-substrate
  - Kubernetes SIGs, Agent Sandbox project site. https://agent-sandbox.sigs.k8s.io/
- **Anthropic: gVisor for claude.ai, a full VM for Cowork's code execution.**
  - Anthropic Engineering, "How we contain Claude across products", 2026-05-25. https://www.anthropic.com/engineering/how-we-contain-claude
- **Fly.io: Firecracker microVMs.**
  - Fly Docs, "The Fly.io Architecture" ("Application code runs in Firecracker microVMs"), accessed 2026-11-04. https://docs.fly.io/reference/architecture
- **None of this year's major agent escapes broke through a wall.** Cosmonic's reading of the slide 11 sources: each escape went through a channel the environment allowed (a package registry cache proxy, internet access left on, an unfiltered DNS resolver).

## Slide 14: Each escape used an open door

Same four incidents as slide 11; sources above.

- **An allowed package proxy.** Hugging Face timeline, 2026-07-27 (above).
- **Internet access left on.** Anthropic, 2026-07-30 (above).
- **A test network left open.** Al Jazeera, 2026-09-19; IBTimes UK, 2026-09-21 (above).
- **Allowed DNS lookups.** OpenAI Alignment, 2026-09-25 (above).

## Slide 15: So code can't be trusted

- **Rising vulnerabilities: monthly disclosures doubled in 2026.** Google Threat Intelligence Group, 2026-09-30 (above).
- **Agent escapes: even in controlled environments.** The slide 11 sources (above).
- **AI-generated code: running with inherited access.** Framing statement; no specific source.

