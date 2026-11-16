(module
  (func (export "compute") (param $count i32) (result i32)
    (local $i i32) (local $value i32)
    (block $done
      (loop $step
        (br_if $done (i32.ge_u (local.get $i) (local.get $count)))
        (local.set $value
          (i32.add
            (i32.rotl (i32.xor (local.get $value) (local.get $i)) (i32.const 5))
            (i32.const 1013904223)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $step)))
    (local.get $value)))
