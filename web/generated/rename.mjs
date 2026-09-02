export function validate(instance) {
  const e = [];
  if (instance === null || typeof instance !== "object" || Array.isArray(instance)) {
    e.push({instancePath: "", schemaPath: "" + "/properties"});
  } else {
    if (!("_type" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/_type"});
    else {
      if (typeof instance["_type"] !== "string" || !["rename"].includes(instance["_type"])) e.push({instancePath: "" + "/_type", schemaPath: "" + "/properties/_type" + "/enum"});
    }
    if (!("title" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/title"});
    else {
      if (typeof instance["title"] !== "string") e.push({instancePath: "" + "/title", schemaPath: "" + "/properties/title" + "/type"});
    }
    for (const k in instance) {
      if (k !== "_type" && k !== "title") e.push({instancePath: "" + "/" + k, schemaPath: ""});
    }
  }
  return e;
}
