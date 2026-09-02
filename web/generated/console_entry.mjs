export function validate(instance) {
  const e = [];
  if (instance === null || typeof instance !== "object" || Array.isArray(instance)) {
    e.push({instancePath: "", schemaPath: "" + "/properties"});
  } else {
    if (!("id" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/id"});
    else {
      if (typeof instance["id"] !== "string") e.push({instancePath: "" + "/id", schemaPath: "" + "/properties/id" + "/type"});
    }
    if (!("level" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/level"});
    else {
      if (typeof instance["level"] !== "string" || !["log","info","warn","error"].includes(instance["level"])) e.push({instancePath: "" + "/level", schemaPath: "" + "/properties/level" + "/enum"});
    }
    if (!("pageId" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/pageId"});
    else {
      if (typeof instance["pageId"] !== "string") e.push({instancePath: "" + "/pageId", schemaPath: "" + "/properties/pageId" + "/type"});
    }
    if (!("text" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/text"});
    else {
      if (typeof instance["text"] !== "string") e.push({instancePath: "" + "/text", schemaPath: "" + "/properties/text" + "/type"});
    }
    if (!("ts" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/ts"});
    else {
      if (typeof instance["ts"] !== "number" || !Number.isFinite(instance["ts"])) e.push({instancePath: "" + "/ts", schemaPath: "" + "/properties/ts" + "/type"});
    }
    for (const k in instance) {
      if (k !== "id" && k !== "level" && k !== "pageId" && k !== "text" && k !== "ts") e.push({instancePath: "" + "/" + k, schemaPath: ""});
    }
  }
  return e;
}
