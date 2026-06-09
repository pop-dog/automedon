I want to design an LLM agent orchestration tool. The concepts I have in mind:

**Graph structure.** The idea of what we're building is that it is a framework for developers to create workflows.
Workflows are structured as a graph, where **nodes** are executable scripts (could be other workflows) and edges
are effectively a *case* statement based on the output of a node.

```
Node A
    run: "python3 /path/to/some/script.py" # exit 0, exit 1, ... , exit n
    nodes:
        0 (i.e. `exit 0`): Node B
        1: Node C
        ...
        n: Node D
```

As we can see, the concept of the workflow graph is LLM-independent. However, this abstraction can be used to
orchestrate LLMs, e.g., when the `run` command is something like `claude -p <prompt which includes exit codes>`.

In this way, this framework defines a structure for connecting scripts via a state machine, and an interface
that the scripts implement to coordinate with one another.

At this stage, our task is to develop the right mental model for this system. The vocabulary we use MUST be
crisp and precise.