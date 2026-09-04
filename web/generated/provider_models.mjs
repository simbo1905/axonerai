export function validate(instance) {
  const e = [];
  if (instance === null || typeof instance !== "object" || Array.isArray(instance)) {
    e.push({instancePath: "", schemaPath: "" + "/properties"});
  } else {
    if (!("models" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/models"});
    else {
      if (!Array.isArray(instance["models"])) {
        e.push({instancePath: "" + "/models", schemaPath: "" + "/properties/models" + "/elements"});
      } else {
        for (let i = 0; i < instance["models"].length; i++) {
          if (instance["models"][i] === null || typeof instance["models"][i] !== "object" || Array.isArray(instance["models"][i])) {
            e.push({instancePath: "" + "/models" + "/" + i, schemaPath: "" + "/properties/models" + "/elements" + "/properties"});
          } else {
            if (!("context_window" in instance["models"][i])) e.push({instancePath: "" + "/models" + "/" + i, schemaPath: "" + "/properties/models" + "/elements" + "/properties/context_window"});
            else {
              if (typeof instance["models"][i]["context_window"] !== "number" || !Number.isInteger(instance["models"][i]["context_window"]) || instance["models"][i]["context_window"] < 0 || instance["models"][i]["context_window"] > 4294967295) e.push({instancePath: "" + "/models" + "/" + i + "/context_window", schemaPath: "" + "/properties/models" + "/elements" + "/properties/context_window" + "/type"});
            }
            if (!("display" in instance["models"][i])) e.push({instancePath: "" + "/models" + "/" + i, schemaPath: "" + "/properties/models" + "/elements" + "/properties/display"});
            else {
              if (typeof instance["models"][i]["display"] !== "string") e.push({instancePath: "" + "/models" + "/" + i + "/display", schemaPath: "" + "/properties/models" + "/elements" + "/properties/display" + "/type"});
            }
            if (!("id" in instance["models"][i])) e.push({instancePath: "" + "/models" + "/" + i, schemaPath: "" + "/properties/models" + "/elements" + "/properties/id"});
            else {
              if (typeof instance["models"][i]["id"] !== "string") e.push({instancePath: "" + "/models" + "/" + i + "/id", schemaPath: "" + "/properties/models" + "/elements" + "/properties/id" + "/type"});
            }
            if ("costs" in instance["models"][i]) {
              if (instance["models"][i]["costs"] !== null) {
                if (instance["models"][i]["costs"] === null || typeof instance["models"][i]["costs"] !== "object" || Array.isArray(instance["models"][i]["costs"])) {
                  e.push({instancePath: "" + "/models" + "/" + i + "/costs", schemaPath: "" + "/properties/models" + "/elements" + "/optionalProperties/costs" + "/optionalProperties"});
                } else {
                  if ("input_per_mtok" in instance["models"][i]["costs"]) {
                    if (typeof instance["models"][i]["costs"]["input_per_mtok"] !== "string") e.push({instancePath: "" + "/models" + "/" + i + "/costs" + "/input_per_mtok", schemaPath: "" + "/properties/models" + "/elements" + "/optionalProperties/costs" + "/optionalProperties/input_per_mtok" + "/type"});
                  }
                  if ("output_per_mtok" in instance["models"][i]["costs"]) {
                    if (typeof instance["models"][i]["costs"]["output_per_mtok"] !== "string") e.push({instancePath: "" + "/models" + "/" + i + "/costs" + "/output_per_mtok", schemaPath: "" + "/properties/models" + "/elements" + "/optionalProperties/costs" + "/optionalProperties/output_per_mtok" + "/type"});
                  }
                  for (const k in instance["models"][i]["costs"]) {
                    if (k !== "input_per_mtok" && k !== "output_per_mtok") e.push({instancePath: "" + "/models" + "/" + i + "/costs" + "/" + k, schemaPath: "" + "/properties/models" + "/elements" + "/optionalProperties/costs"});
                  }
                }
              }
            }
            if ("offer" in instance["models"][i]) {
              if (typeof instance["models"][i]["offer"] !== "string") e.push({instancePath: "" + "/models" + "/" + i + "/offer", schemaPath: "" + "/properties/models" + "/elements" + "/optionalProperties/offer" + "/type"});
            }
            for (const k in instance["models"][i]) {
              if (k !== "context_window" && k !== "display" && k !== "id" && k !== "costs" && k !== "offer") e.push({instancePath: "" + "/models" + "/" + i + "/" + k, schemaPath: "" + "/properties/models" + "/elements"});
            }
          }
        }
      }
    }
    if (!("provider" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/provider"});
    else {
      if (typeof instance["provider"] !== "string") e.push({instancePath: "" + "/provider", schemaPath: "" + "/properties/provider" + "/type"});
    }
    if (!("updated" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/updated"});
    else {
      if (typeof instance["updated"] !== "string") e.push({instancePath: "" + "/updated", schemaPath: "" + "/properties/updated" + "/type"});
    }
    for (const k in instance) {
      if (k !== "models" && k !== "provider" && k !== "updated") e.push({instancePath: "" + "/" + k, schemaPath: ""});
    }
  }
  return e;
}
