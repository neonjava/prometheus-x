export function mountHow(root) {
  root.innerHTML = `
    <div class="page-head"><div><h1>How it works</h1><p>Learn what good looks like, then flag anything that does not match.</p></div></div>
    <div class="grid-2">
      <div><img class="flow-img" src="workflow.png" alt="Photo goes to a quick check, then a closer look, then a person if the checks disagree"></div>
      <div class="steps" style="margin-top:0">
        <div class="step"><b>Quick check</b><p>The whole photo is compared with every good photo it learned from. Normal photos pass here and go no further.</p></div>
        <div class="step"><b>Closer look</b><p>Odd photos are checked again, patch by patch. Both checks must agree: damage is rejected and shown on a heatmap, with how bad and how sure.</p></div>
        <div class="step"><b>A person decides</b><p>If the checks disagree, the photo is held for review. OK, Reject or Fix is saved so there is a record.</p></div>
        <div class="step"><b>Lighting or camera changed?</b><p>If a photo looks different from the training photos, a warning is shown so the result is not trusted blindly.</p></div>
      </div>
    </div>`;
  return null;
}
