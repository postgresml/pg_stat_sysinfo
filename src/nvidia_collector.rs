use nvml_wrapper::Nvml;
use nvml_wrapper::enum_wrappers::device::{
    TemperatureSensor,
};


pub type GPUResponse = (i32, String, f64, f64, f64, i32, pgrx::JsonB, i32, i32);



pub fn get_cuda_information() -> Vec<GPUResponse> {
    let nvml = Nvml::init().unwrap();
    let device_count  = nvml.device_count().unwrap();
    let mut records: Vec<GPUResponse> = Vec::new();

    for i in 0..device_count {
        let device = nvml.device_by_index(i).unwrap();
        let device_name = device.name().unwrap();
        let index = device.index().unwrap() as i32;
        let memory_info = device.memory_info().unwrap();
        let total_memory = memory_info.total as f64/  1000000.00; // in mb
        let free_memory = memory_info.free as f64/  1000000.00; // in mb
        let used_memory = memory_info.used as f64/  1000000.00; // in mb
        let temperature = device.temperature(TemperatureSensor::Gpu).unwrap() as i32;

        // let process_info = device.running_compute_processes().unwrap();

        let utilization = device.utilization_rates().unwrap();

        let process_infos: serde_json::Value = serde_json::json!(
         device
            .running_compute_processes()
            .unwrap()
            .into_iter()
            .map(|process_info| {
                serde_json::json!({
                    "pid": process_info.pid,
                    "gpu_instance_id": process_info.gpu_instance_id,
                    "compute_instance_id": process_info.compute_instance_id,
                })
            })
            .collect::<Vec<serde_json::Value>>()
        );


        let record = (
            index, device_name, total_memory, free_memory,
            used_memory, temperature, pgrx::JsonB(process_infos),
            utilization.gpu as i32, utilization.memory as i32
        );

        records.push(record);

    }
    records
}
