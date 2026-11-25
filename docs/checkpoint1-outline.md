# Checkpoint 1 outline

1. **Problem.** Find defects on PCB images when only good images exist, and keep working when lighting, camera or batch changes.
2. **Solution.** Learn what good looks like; anything far from it is suspicious. Two checks on one ResNet-18, and a person reviews only the doubtful cases.
3. **Approach.** Step 1 quick global check; Step 2 patch check for small local defects; Step 3 human decision. Diagram: `workflow.png`.
4. **Architecture.** `docs/ARCHITECTURE.md`: config, features, data, analysis, detector, cli, server, web.
5. **Stack.** Rust, candle, `image`, axum, plain HTML/JS.
6. **Progress.** Working end to end on VisA pcb1 (AUROC 0.985, no good image called a defect). Next: lighting-change tests, more datasets, accuracy tuning.
