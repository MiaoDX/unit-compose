# Milestone 3 allocation policies

Status: implemented

Capacity behavior and allocation guarantees are orthogonal. The public
constructors provide development (`grow-and-measure`, best effort) and strict
(`reject-overflow`, no run allocation) presets, while `BuildOptions::try_new`
rejects growth combined with a no-run-allocation claim before Unit storage or
workspace preparation begins.

Strict preparation accepts only fixed or bounded requirements. Every declared
allocation domain must be instrumented or certified by a non-empty, inspectable
source. Certification is a trusted Unit, adapter, or host-integrator assertion;
neither the framework nor this conformance suite proves that arbitrary native
code declared every allocator it can call.

`Module::run_profiled` requires probes for every instrumented declared domain
and places Resource reset, Unit and helper execution,
pending-output validation and cleanup, and registered diagnostic sinks inside
the probe boundary. Adapter probes identify their domain and return allocate,
reallocate, and deallocate totals. Any activity becomes a structured
allocation-profile violation. A probe for an undeclared or non-instrumented
domain is rejected. Module descriptions expose the policy, requirement status,
domain evidence, certification source, and the warm-up boundary.

Construction and explicitly declared warm-up are outside the measured run
boundary. The isolated allocation harness warms once, then measures 1,000
steady-state strict runs using a thread-local scoped global allocator counter;
other test threads cannot affect its totals. Its single test also covers
success, recoverable and fatal Unit failures, overflow, Resource reset, pending
cleanup, helper code reached by the synthetic Units, diagnostic sinks, an
allocating Unit, and an uninstrumented adapter probe.

Run reports use a fixed event array. They expose observed capacity peaks,
bounded event loss, and allocation-operation totals without allocating on the
strict path. Overflow remains a structured error and never grows prepared
storage.
