const state = visual.state({ metric: 0, scale: 1, comparison: true });
const labels = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const baseline = [42, 56, 48, 72, 65, 83, 91];

return () => {
  const values = baseline.map(value => Math.round(value * state.scale));
  const series = [{ label: "Current", values, color: "accent" }];
  if (state.comparison) {
    series.push({ label: "Previous", values: [35, 48, 44, 59, 62, 67, 73], color: "series2" });
  }
  return visual.column([
    visual.text("Throughput explorer", { size: "title" }),
    visual.text("Explore a sample week with native charts and controls.", { color: "muted" }),
    visual.grid([
      visual.card([visual.stat("Total", values.reduce((sum, value) => sum + value, 0), {
        delta: "+24%", detail: "versus the previous week", trend: values })]),
      visual.card([visual.gauge("Peak load", Math.round(Math.max(...values) / 2), { unit: "%" })]),
      visual.card([visual.donut([
        { label: "Search", value: 48 }, { label: "Checkout", value: 27 }, { label: "Accounts", value: 16 },
      ], { label: "Requests" })]),
    ], { columns: 3, gap: 16 }),
    visual.tabs(["Bars", "Trend", "Area"], state.metric, value => { state.metric = value; }),
    visual.chart(["bar", "line", "area"][state.metric], series, { labels, height: 240 }),
    visual.slider("Scenario multiplier", state.scale, 0.5, 2, value => { state.scale = value; }, { step: 0.1 }),
    visual.toggle("Compare with previous week", state.comparison, value => { state.comparison = value; }),
    visual.table(["Day", "Requests"], labels.map((label, index) => [label, String(values[index])])),
    visual.button("Reset scenario", () => {
      state.scale = 1;
      state.metric = 0;
      state.comparison = true;
    }),
  ], { gap: 16 });
};
