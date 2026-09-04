export function validate(instance) {
  const e = [];
  if (instance === null || typeof instance !== "object" || Array.isArray(instance)) {
    e.push({instancePath: "", schemaPath: "" + "/properties"});
  } else {
    if (!("_type" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/_type"});
    else {
      if (typeof instance["_type"] !== "string" || !["model_changed"].includes(instance["_type"])) e.push({instancePath: "" + "/_type", schemaPath: "" + "/properties/_type" + "/enum"});
    }
    if (!("model" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/model"});
    else {
      if (typeof instance["model"] !== "string") e.push({instancePath: "" + "/model", schemaPath: "" + "/properties/model" + "/type"});
    }
    if (!("service" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/service"});
    else {
      if (typeof instance["service"] !== "string") e.push({instancePath: "" + "/service", schemaPath: "" + "/properties/service" + "/type"});
    }
    if (!("ts" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/ts"});
    else {
      if (typeof instance["ts"] !== "string") e.push({instancePath: "" + "/ts", schemaPath: "" + "/properties/ts" + "/type"});
    }
    for (const k in instance) {
      if (k !== "_type" && k !== "model" && k !== "service" && k !== "ts") e.push({instancePath: "" + "/" + k, schemaPath: ""});
    }
  }
  return e;
}
