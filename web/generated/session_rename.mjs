export function validate(instance) {
  const e = [];
  if (instance === null || typeof instance !== "object" || Array.isArray(instance)) {
    e.push({instancePath: "", schemaPath: "" + "/properties"});
  } else {
    if (!("_type" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/_type"});
    else {
      if (typeof instance["_type"] !== "string" || !["session_rename"].includes(instance["_type"])) e.push({instancePath: "" + "/_type", schemaPath: "" + "/properties/_type" + "/enum"});
    }
    if (!("title" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/title"});
    else {
      if (typeof instance["title"] !== "string") e.push({instancePath: "" + "/title", schemaPath: "" + "/properties/title" + "/type"});
    }
    if (!("ts" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/ts"});
    else {
      if (typeof instance["ts"] !== "number" || !Number.isFinite(instance["ts"])) e.push({instancePath: "" + "/ts", schemaPath: "" + "/properties/ts" + "/type"});
    }
    for (const k in instance) {
      if (k !== "_type" && k !== "title" && k !== "ts") e.push({instancePath: "" + "/" + k, schemaPath: ""});
    }
  }
  return e;
}
