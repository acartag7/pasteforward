#!/usr/bin/env ruby
# frozen_string_literal: true

require "yaml"

MAX_WORKFLOW_BYTES = 1024 * 1024
MAX_NODES = 10_000
MAX_DEPTH = 100
PINNED_REMOTE = /\A[^\s@]+@[0-9a-f]{40}\z/

def valid_action_target?(target)
  target.is_a?(String) && (target.start_with?("./") || PINNED_REMOTE.match?(target))
end

def check_node(node, seen, count, depth)
  raise "workflow structure exceeds limits" if depth > MAX_DEPTH || count[0] >= MAX_NODES

  count[0] += 1
  return unless node.is_a?(Hash) || node.is_a?(Array)
  return if seen[node.object_id]

  seen[node.object_id] = true
  if node.is_a?(Hash)
    node.each do |key, value|
      raise "workflow action reference is not pinned" if key == "uses" && !valid_action_target?(value)

      check_node(value, seen, count, depth + 1)
    end
  else
    node.each { |value| check_node(value, seen, count, depth + 1) }
  end
end

def check_document(source)
  document = YAML.safe_load(source, permitted_classes: [], permitted_symbols: [], aliases: false)
  check_node(document, {}, [0], 0)
end

def self_test
  rejected = [
    "steps:\n  - uses: owner/action@v4 # uses: decoy/action@#{'0' * 40}\n",
    "steps:\n  - \"uses\": owner/action@v4\n",
    "steps: [ { uses: owner/action@v4 } ]\n",
    "jobs:\n  call:\n    uses: owner/repo/.github/workflows/check.yml@main\n",
    "steps: &steps\n  - uses: owner/action@#{'0' * 40}\ncopy: *steps\n"
  ]
  rejected.each do |source|
    begin
      check_document(source)
    rescue Psych::Exception, RuntimeError
      next
    end
    raise "workflow pin checker self-test failed"
  end
  check_document("steps:\n  - uses: ./local-action\n  - uses: owner/action@#{'0' * 40}\n")
  check_document("steps:\n  # - uses: owner/action@v4\n")
end

begin
  self_test
  Dir[".github/workflows/*.{yml,yaml}"].sort.each do |path|
    raise "workflow link is not allowed" if File.lstat(path).symlink?
    raise "workflow file exceeds size limit" if File.size(path) > MAX_WORKFLOW_BYTES

    check_document(File.binread(path))
  end
rescue Psych::Exception, RuntimeError, SystemCallError
  warn "workflow action pinning check failed"
  exit 1
end
