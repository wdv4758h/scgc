#[macro_use]
extern crate scgc;
#[macro_use]
extern crate log;
extern crate env_logger;


fn main() {
    env_logger::init().unwrap();

    let mut gc = scgc::Gc::new(20480);

    let x = true;
    gc.stack_begin(&x);

    let data1: &mut [u8; 1024] = malloc!(gc, 4096);
    data1[0] = 42;
    debug!("pointer on stack {:p} -> data on heap {:p}", &data1, data1);
    let mut data2: &mut [u8; 1024] = &mut [0; 1024];

    for i in 1..100 {
        let mut data: &mut [u8; 1024] = malloc!(gc, 4096);
        data[0] = i;
        assert_eq!(data2[0], i-1);   // test the cleanup won't accidentally reuse data2
        data2 = data;
        assert_eq!(data1[0], 42);    // test the cleanup won't accidentally reuse data1
    }
}
